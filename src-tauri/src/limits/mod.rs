//! Subscription rate limits aggregated per account from provider-owned clients/endpoints,
//! remembered snapshots, and account-correlated local observations. Claude's stored login is read
//! here and renewed in one place only, [`claude_renew`], when its access token has expired and can
//! still renew itself; Codex owns its authentication and refresh lifecycle through app-server.
//!
//! `read_limits` never fails for provider-side reasons; every outcome is a `ProviderLimitsDto`
//! whose `status` + `message` tell the UI what to show. Because each CLI stores one login at a
//! time, successful reads are also remembered per account (numbers only) so accounts the user
//! has switched away from stay visible with each window's observation time.

mod backend_memo;
mod claude;
use crate::accounts::claude_renew;
mod codex;
mod codex_app_server;
mod codex_sessions;
pub(crate) mod credentials;
pub(crate) mod credits_spent;
pub(crate) mod json;
pub(crate) mod login;
mod pipeline;
mod reading;
mod renewal;
pub(crate) mod saved;
mod snapshots;

pub(crate) use reading::keep_remembered;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;

use crate::dto::{
    AgentId, LimitsAccountDto, LimitsStatus, ProviderLimitsDto, Reading, ResetCreditOutcome,
};
use crate::http::{get_json, HttpError};
use crate::paths;
use credentials::{
    read_claude_identity, ClaudeCredential, ClaudeIdentity, ClaudeLoginMemo, CredentialLookup,
    KeychainProbe, LoginSource, CLAUDE_LOGIN,
};
#[cfg(test)]
use pipeline::resolve;
use pipeline::{finish, resolve_provider, LoadFailureKind, ProviderLoadError, ResolveOutcome};
use snapshots::SnapshotStore;

const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// Asks the usage read for the saved-reset block too. The query is the one Claude Code sends on
/// demand for `/limit-reset`; its regular read is the plain URL, which is why [`claude_usage`] falls
/// back to it. `skip_spend` leaves out the extra-usage spend figures, which on-n-off does not show.
const CLAUDE_USAGE_QUERY: &str = "cedar_ember=1&skip_spend=1";
const CLAUDE_PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
/// Account id used when the CLI stores no identity; keeps single-account behaviour intact.
const DEFAULT_ACCOUNT: &str = "default";
static CLAUDE_READ_LOCK: Mutex<()> = Mutex::new(());
static CODEX_READ_LOCK: Mutex<()> = Mutex::new(());

/// A parsed usage read: which account it is about, and its reading. `Default` is the empty read
/// that accompanies every non-`ok` status.
#[derive(Debug, Clone, Default, PartialEq)]
struct Parsed {
    account: Option<LimitsAccountDto>,
    reading: Reading,
}

#[cfg(test)]
impl Parsed {
    /// A read for one card, with only what the backend reads decide by: its account and plan.
    pub(super) fn for_card(account: Option<&str>, plan: Option<&str>) -> Self {
        Self {
            account: account.map(|id| LimitsAccountDto {
                legacy_id: None,
                id: id.to_string(),
                label: None,
            }),
            reading: Reading {
                plan: plan.map(str::to_string),
                ..Reading::default()
            },
        }
    }
}

/// The signed-in Codex card an app-server `account/rateLimits/read` result becomes, each window
/// observed at `observed_at`: the reader's own parse and card, for tests elsewhere that need what
/// the reader keeps of a read.
#[cfg(test)]
pub(crate) fn codex_card(
    rate_limits: serde_json::Value,
    account_id: &str,
    observed_at: &str,
) -> ProviderLimitsDto {
    let payload = serde_json::from_value(rate_limits).expect("an app-server rate-limits result");
    let mut reading = codex::parse_codex(&payload);
    for window in &mut reading.windows {
        window.observed_at = observed_at.to_string();
    }
    finish(
        AgentId::Codex,
        LimitsStatus::Ok,
        None,
        Parsed {
            account: Some(LimitsAccountDto {
                legacy_id: None,
                id: account_id.to_string(),
                label: None,
            }),
            reading,
        },
    )
}

/// The three services one Claude read talks to, together so adding a fourth costs one field and
/// not an edit at every call site.
#[derive(Debug, Clone, Copy)]
struct ClaudeEndpoints<'a> {
    /// Where a stale access token is renewed, before anything is asked of the other two.
    token: &'a str,
    profile: &'a str,
    usage: &'a str,
}

/// Everything `read_limits` needs that tests replace: where the homes/snapshots live, the Claude
/// Keychain probe and memo, and the Claude endpoints.
struct Sources<'a, P: Fn() -> KeychainProbe> {
    home: &'a Path,
    memo: &'a ClaudeLoginMemo,
    keychain: P,
    claude: ClaudeEndpoints<'a>,
    now_ms: i64,
}

/// Current subscription limits for one provider, followed by remembered observations for its other
/// accounts. Blocking: runs a Keychain probe and HTTPS requests for Claude or a bounded Codex
/// app-server process; call it off the UI thread. `force` requests fresh provider authentication.
pub fn read_limits(agent: AgentId, force: bool) -> Vec<ProviderLimitsDto> {
    let home = match paths::user_home() {
        Ok(home) => home,
        Err(error) => {
            return vec![finish(
                agent,
                LimitsStatus::Failed,
                Some(format!(
                    "Could not read the stored login: {}",
                    error.message
                )),
                Parsed::default(),
            )]
        }
    };
    read_limits_in(
        agent,
        force,
        Sources {
            home: &home,
            memo: &CLAUDE_LOGIN,
            keychain: credentials::keychain_claude_json,
            claude: ClaudeEndpoints {
                token: claude_renew::TOKEN_URL,
                profile: CLAUDE_PROFILE_URL,
                usage: CLAUDE_USAGE_URL,
            },
            now_ms: Utc::now().timestamp_millis(),
        },
    )
}

/// Spend one banked Codex reset on the signed-in account `account_id` names. Blocking: holds the
/// Codex read lock for one bounded app-server call, so it never overlaps a limits read.
pub fn consume_codex_reset_credit(
    account_id: &str,
    idempotency_key: &str,
) -> Result<ResetCreditOutcome, String> {
    let home = paths::user_home().map_err(|error| error.message)?;
    let _provider_guard = provider_read_guard(AgentId::Codex);
    codex_app_server::consume_reset_credit(&home, account_id, idempotency_key)
}

/// Drop the remembered snapshot of one account (the user's "Forget" on a remembered card).
pub fn forget_snapshot(
    agent: AgentId,
    account_id: &str,
    expected_email: Option<&str>,
) -> Result<(), String> {
    let home = paths::user_home().map_err(|error| error.message)?;
    if let Some(email) = expected_email {
        // Legacy cleanup is conditional numeric history removal.
        return SnapshotStore::for_home(&home).forget_matching_email(agent, account_id, email);
    }
    SnapshotStore::for_home(&home).forget(agent, account_id)
}

fn read_limits_in<P: Fn() -> KeychainProbe>(
    agent: AgentId,
    force: bool,
    sources: Sources<'_, P>,
) -> Vec<ProviderLimitsDto> {
    let _provider_guard = match agent {
        AgentId::Claude | AgentId::Codex => Some(provider_read_guard(agent)),
        AgentId::Antigravity | AgentId::Cursor => None,
    };
    let home = sources.home;
    let observed_at =
        chrono::DateTime::<Utc>::from_timestamp_millis(sources.now_ms).unwrap_or_else(Utc::now);
    let current = match agent {
        AgentId::Claude => claude_current(force, sources),
        AgentId::Codex => codex_limits(home, force),
        AgentId::Antigravity | AgentId::Cursor => {
            return vec![finish(
                agent,
                LimitsStatus::Unsupported,
                Some(format!(
                    "{} has no subscription limits to show.",
                    agent.display_name()
                )),
                Parsed::default(),
            )]
        }
    };
    let store = SnapshotStore::for_home(home);
    let mut accounts = aggregate_accounts(&store, current);
    if agent == AgentId::Codex {
        let before = accounts.clone();
        if codex_sessions::merge_recent(home, observed_at, &mut accounts) > 0 {
            store.save_changed(&before, &accounts);
        }
    }
    accounts
}

fn provider_read_lock(agent: AgentId) -> &'static Mutex<()> {
    match agent {
        AgentId::Claude => &CLAUDE_READ_LOCK,
        AgentId::Codex => &CODEX_READ_LOCK,
        AgentId::Antigravity | AgentId::Cursor => {
            unreachable!("unsupported providers do not run a limits read")
        }
    }
}

fn provider_read_guard(agent: AgentId) -> MutexGuard<'static, ()> {
    provider_read_lock(agent)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The current account's card, keeping what its read could not tell from the account's remembered
/// reading, persisted; then the other remembered accounts, newest first.
fn aggregate_accounts(
    store: &SnapshotStore,
    mut current: ProviderLimitsDto,
) -> Vec<ProviderLimitsDto> {
    let current_account = current.account.as_ref().map(|account| account.id.clone());
    let mut remembered = store.load(current.provider);
    let prior = current_account.as_deref().and_then(|id| {
        remembered
            .iter()
            .position(|snapshot| {
                snapshot
                    .account
                    .as_ref()
                    .is_some_and(|account| account.id == id)
            })
            .map(|index| remembered.remove(index))
    });
    keep_remembered(
        &mut current,
        prior.map(|prior| prior.reading).unwrap_or_default(),
    );
    let _ = store.save(&current);
    snapshots::without_superseded(std::iter::once(current).chain(remembered).collect())
}

/// Claude: which account the CLI is signed into (`~/.claude.json`) decides whether the memoised
/// login may be reused; otherwise the Keychain (or the credentials file) is read. A rejected token
/// evicts the memo so the next read goes back to the Keychain.
fn claude_current<P: Fn() -> KeychainProbe>(
    force: bool,
    sources: Sources<'_, P>,
) -> ProviderLimitsDto {
    let Sources {
        home,
        memo,
        keychain,
        claude,
        now_ms,
        ..
    } = sources;
    let selected_identity = read_claude_identity(home);
    let account = selected_identity
        .as_ref()
        .map(|identity| identity.account.clone())
        .unwrap_or_else(default_account);
    // Reading the Claude login includes renewing it: an access token lives eight hours and Claude
    // Code renews it only while it is running, so a longer gap is the ordinary case rather than a
    // broken login. Keeping that inside the read means the memo stores the renewed login like any
    // other, and the rejected-token retry below gets the renewal too.
    let read_credential = || claude_renew::current_login(home, &keychain, now_ms, claude.token);
    let attempt = |lookup| claude_limits(lookup, &selected_identity, claude.profile, claude.usage);
    let (lookup, source) = memo.lookup(force, &account.id, now_ms, read_credential);
    let mut loaded = attempt(lookup);
    // Claude Code rotates the access token before the expiry it records, which leaves the memo
    // holding one the endpoint has already stopped accepting. That rejection says nothing about
    // the login, so read the stored login again and try once more before reporting one —
    // otherwise a signed-in user is told to sign in again until the next poll.
    //
    // A re-read that hands back no login leaves the rejection standing, because the rejection is
    // the accurate answer and the alternatives are louder falsehoods: `Missing` would report
    // "Sign in with `claude`" at a user who is signed in, and `Unreadable` a Keychain failure
    // when the prompt this retry raised went unanswered. `Expired` is the one where the discarded
    // message would have been gentler ("send a prompt to renew it"), but it describes a login
    // this attempt never sent; reporting what the endpoint actually refused is the honest answer.
    if source == LoginSource::Memo && loaded.failure == Some(LoadFailureKind::Unauthorized) {
        if let Some(fresh) = memo.refreshed(&account.id, now_ms, read_credential) {
            loaded = attempt(CredentialLookup::Found(fresh));
        }
    }
    if loaded.dto.status == LimitsStatus::Unauthenticated
        || loaded.failure == Some(LoadFailureKind::AccountMismatch)
    {
        memo.clear();
    }
    if let (Some(identity), Some(account)) = (&selected_identity, &mut loaded.dto.account) {
        if let Some(workspace) = &identity.organization_id {
            account.legacy_id = Some(identity.account.id.clone());
            account.id = crate::accounts::model::Identity {
                provider: AgentId::Claude,
                user_id: identity.account.id.clone(),
                workspace_id: workspace.clone(),
            }
            .observation_key();
        }
    }
    loaded.dto
}

/// Claude: verify the stored token's profile, then read usage with the OAuth beta header. The plan
/// label comes from the login itself (`subscriptionType` and known Max `rateLimitTier` values).
fn claude_limits(
    lookup: CredentialLookup<ClaudeCredential>,
    selected_identity: &Option<ClaudeIdentity>,
    profile_url: &str,
    usage_url: &str,
) -> ResolveOutcome {
    let selected_account = selected_identity
        .as_ref()
        .map(|identity| identity.account.clone())
        .unwrap_or_else(default_account);
    resolve_provider(
        AgentId::Claude,
        Some(selected_account),
        lookup,
        |credential| {
            let bearer = format!("Bearer {}", credential.token);
            let profile_payload = get_json(
                profile_url,
                &[
                    ("Authorization", &bearer),
                    ("Content-Type", "application/json"),
                    ("Cache-Control", "no-cache"),
                ],
            )?;
            let claude::ClaudeProfile {
                identity: profile,
                subscription_status,
            } = claude::parse_profile(&profile_payload).map_err(HttpError::Parse)?;
            if selected_identity.as_ref().is_some_and(|selected| {
                selected.account.id != profile.account.id
                    || selected.organization_id != profile.organization_id
            }) {
                return Err(ProviderLoadError::AccountMismatch);
            }
            let usage = claude_usage(
                usage_url,
                &[
                    ("Authorization", &bearer),
                    ("anthropic-beta", "oauth-2025-04-20"),
                    ("Cache-Control", "no-cache"),
                ],
            )?;
            Ok(Parsed {
                account: Some(profile.account),
                reading: Reading {
                    plan: credential.plan(),
                    subscription_status,
                    ..usage
                },
            })
        },
    )
}

/// Claude's usage read with its saved resets. The resets are optional, so a refusal of the reset
/// query never decides the read: any answer other than a transport failure retries the plain URL,
/// whose answer (a rejected login included) stands, with the resets unknown. A transport failure
/// is not retried, since the plain read would only wait on the same network.
fn claude_usage(usage_url: &str, headers: &[(&str, &str)]) -> Result<Reading, HttpError> {
    let payload = match get_json(&format!("{usage_url}?{CLAUDE_USAGE_QUERY}"), headers) {
        Err(HttpError::Network(error)) => return Err(HttpError::Network(error)),
        Err(_) => get_json(usage_url, headers)?,
        Ok(payload) => payload,
    };
    Ok(claude::parse_usage(&payload, Utc::now()))
}

/// Codex owns login, token refresh and usage requests through its documented app-server APIs.
fn codex_limits(home: &Path, force: bool) -> ProviderLimitsDto {
    match codex_app_server::read(home, force) {
        Ok(parsed) => finish(AgentId::Codex, LimitsStatus::Ok, None, parsed),
        Err(codex_app_server::AppServerFailure::SignedOut) => finish(
            AgentId::Codex,
            LimitsStatus::SignedOut,
            Some("Sign in with `codex` to see subscription limits.".to_string()),
            Parsed::default(),
        ),
        Err(codex_app_server::AppServerFailure::Unsupported(message)) => finish(
            AgentId::Codex,
            LimitsStatus::Unsupported,
            Some(message),
            Parsed::default(),
        ),
        Err(codex_app_server::AppServerFailure::Failed(message)) => finish(
            AgentId::Codex,
            LimitsStatus::Failed,
            Some(message),
            Parsed::default(),
        ),
    }
}

fn default_account() -> LimitsAccountDto {
    LimitsAccountDto {
        legacy_id: None,
        id: DEFAULT_ACCOUNT.to_string(),
        label: None,
    }
}

pub(crate) fn clear_login_memo() {
    credentials::CLAUDE_LOGIN.clear();
}

#[cfg(test)]
mod tests;
