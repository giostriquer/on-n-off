//! Subscription rate limits aggregated per account from provider-owned clients/endpoints,
//! remembered snapshots, and account-correlated local observations. Claude's stored login remains
//! read-only; Codex owns its authentication and refresh lifecycle through app-server.
//!
//! `read_limits` never fails for provider-side reasons; every outcome is a `ProviderLimitsDto`
//! whose `status` + `message` tell the UI what to show. Because each CLI stores one login at a
//! time, successful reads are also remembered per account (numbers only) so accounts the user
//! has switched away from stay visible with each window's observation time.

mod claude;
mod claude_desktop;
mod codex;
mod codex_app_server;
mod codex_sessions;
mod credentials;
mod json;
mod observations;
mod pipeline;
mod snapshots;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;

use crate::dto::{
    AgentId, LimitWindowDto, LimitsAccountDto, LimitsCreditsDto, LimitsStatus, ProviderLimitsDto,
};
use crate::http::{get_json, HttpError};
use crate::paths;
use credentials::{
    read_claude_credential, read_claude_identity, ClaudeCredential, ClaudeIdentity,
    ClaudeLoginMemo, CredentialLookup, KeychainProbe, LoginSource, CLAUDE_LOGIN,
};
use observations::ObservedWindowSet;
#[cfg(test)]
use pipeline::resolve;
use pipeline::{finish, resolve_provider, LoadFailureKind, ProviderLoadError, ResolveOutcome};
use snapshots::SnapshotStore;

const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CLAUDE_PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
/// Account id used when the CLI stores no identity; keeps single-account behaviour intact.
const DEFAULT_ACCOUNT: &str = "default";
static CLAUDE_READ_LOCK: Mutex<()> = Mutex::new(());
static CODEX_READ_LOCK: Mutex<()> = Mutex::new(());

/// Provider-neutral content of a parsed usage payload; `Default` is the empty snapshot that
/// accompanies every non-`ok` status.
#[derive(Debug, Clone, Default, PartialEq)]
struct Parsed {
    account: Option<LimitsAccountDto>,
    plan: Option<String>,
    windows: Vec<LimitWindowDto>,
    credits: Option<LimitsCreditsDto>,
}

/// Everything `read_limits` needs that tests replace: where the homes/snapshots live, the Claude
/// Keychain probe and memo, and the Claude endpoint.
struct Sources<'a, P: Fn() -> KeychainProbe> {
    home: &'a Path,
    memo: &'a ClaudeLoginMemo,
    keychain: P,
    claude_profile_url: &'a str,
    claude_url: &'a str,
    claude_desktop_history: PathBuf,
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
    let claude_desktop_history = claude_desktop::history_path(&home);
    read_limits_in(
        agent,
        force,
        Sources {
            home: &home,
            memo: &CLAUDE_LOGIN,
            keychain: credentials::keychain_claude_json,
            claude_profile_url: CLAUDE_PROFILE_URL,
            claude_url: CLAUDE_USAGE_URL,
            claude_desktop_history,
            now_ms: Utc::now().timestamp_millis(),
        },
    )
}

/// Drop the remembered snapshot of one account (the user's "Forget" on a remembered card).
pub fn forget_snapshot(agent: AgentId, account_id: &str) -> Result<(), String> {
    let home = paths::user_home().map_err(|error| error.message)?;
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
    let (current, supplemental) = match agent {
        AgentId::Claude => claude_current(force, sources),
        AgentId::Codex => (codex_limits(home, force), None),
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
    let mut accounts = aggregate_accounts(&store, current, supplemental);
    if agent == AgentId::Codex && codex_sessions::merge_recent(home, observed_at, &mut accounts) > 0
    {
        for account in &accounts {
            let _ = store.save(account);
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

/// Merge every available observation for the current account, persist the canonical account view,
/// then return it followed by the other remembered accounts, newest first.
fn aggregate_accounts(
    store: &SnapshotStore,
    current: ProviderLimitsDto,
    supplemental: Option<ObservedWindowSet>,
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
    let current = if current.status == LimitsStatus::Ok {
        current
    } else {
        observations::merge_windows(
            current,
            supplemental,
            prior.and_then(ObservedWindowSet::from_account),
        )
    };
    let _ = store.save(&current);
    std::iter::once(current).chain(remembered).collect()
}

/// Claude: which account the CLI is signed into (`~/.claude.json`) decides whether the memoised
/// login may be reused; otherwise the Keychain (or the credentials file) is read. A rejected token
/// evicts the memo so the next read goes back to the Keychain.
fn claude_current<P: Fn() -> KeychainProbe>(
    force: bool,
    sources: Sources<'_, P>,
) -> (ProviderLimitsDto, Option<ObservedWindowSet>) {
    let Sources {
        home,
        memo,
        keychain,
        claude_profile_url,
        claude_url,
        claude_desktop_history,
        now_ms,
        ..
    } = sources;
    let selected_identity = read_claude_identity(home);
    let account = selected_identity
        .as_ref()
        .map(|identity| identity.account.clone())
        .unwrap_or_else(default_account);
    let organization_id = selected_identity
        .as_ref()
        .and_then(|identity| identity.organization_id.clone());
    let read_credential = || read_claude_credential(home, keychain(), now_ms);
    let (lookup, source) = memo.lookup(force, &account.id, now_ms, read_credential);
    let mut loaded = claude_limits(
        lookup,
        selected_identity.clone(),
        claude_profile_url,
        claude_url,
    );
    // Claude Code rotates the access token before its recorded expiry, which leaves the memo
    // holding one the endpoint has already stopped accepting. That rejection says nothing about
    // the login, so read the stored login again and try once more before reporting one — the
    // alternative is telling a signed-in user to sign in again until the next poll.
    if source == LoginSource::Memo && loaded.failure == Some(LoadFailureKind::Unauthorized) {
        let (fresh, _) = memo.lookup(true, &account.id, now_ms, read_credential);
        loaded = claude_limits(
            fresh,
            selected_identity.clone(),
            claude_profile_url,
            claude_url,
        );
    }
    if loaded.dto.status == LimitsStatus::Unauthenticated
        || loaded.failure == Some(LoadFailureKind::AccountMismatch)
    {
        memo.clear();
    }
    let local_observations = (loaded.dto.status != LimitsStatus::Ok)
        .then(|| {
            organization_id
                .as_deref()
                .and_then(|organization_id| {
                    claude_desktop::read_latest(&claude_desktop_history, organization_id)
                })
                .map(|usage| ObservedWindowSet::local(usage.observed_at, usage.windows))
        })
        .flatten();
    (loaded.dto, local_observations)
}

/// Claude: verify the stored token's profile, then read usage with the OAuth beta header. The plan
/// label comes from the login itself (`subscriptionType`).
fn claude_limits(
    lookup: CredentialLookup<ClaudeCredential>,
    selected_identity: Option<ClaudeIdentity>,
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
            let profile = claude::parse_profile(&profile_payload).map_err(HttpError::Parse)?;
            if selected_identity.as_ref().is_some_and(|selected| {
                selected.account.id != profile.account.id
                    || selected.organization_id != profile.organization_id
            }) {
                return Err(ProviderLoadError::AccountMismatch);
            }
            let payload = get_json(
                usage_url,
                &[
                    ("Authorization", &bearer),
                    ("anthropic-beta", "oauth-2025-04-20"),
                    ("Cache-Control", "no-cache"),
                ],
            )?;
            Ok(Parsed {
                account: Some(profile.account),
                plan: credential.subscription_type.clone(),
                windows: claude::parse_claude(&payload),
                credits: None,
            })
        },
    )
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
        id: DEFAULT_ACCOUNT.to_string(),
        label: None,
    }
}

#[cfg(test)]
mod tests;
