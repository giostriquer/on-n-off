//! Subscription rate limits aggregated per account from provider-owned clients/endpoints,
//! remembered snapshots, and account-correlated local observations. Claude's stored login is read by
//! its reader (`claude.rs`) and renewed in one place only,
//! [`claude_renew`](crate::accounts::claude_renew), when its access token has expired and can still
//! renew itself; Codex owns its authentication and refresh lifecycle through app-server, which its
//! reader (`codex.rs`) asks.
//!
//! `read_limits` never fails for provider-side reasons; every outcome is a `ProviderLimitsDto`
//! whose `status` + `message` tell the UI what to show. Because each CLI stores one login at a
//! time, successful reads are also remembered per account (numbers only) so accounts the user
//! has switched away from stay visible with each window's observation time.

mod backend_memo;
mod claude;
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
mod snapshots;

pub(crate) use claude::{claude_headers, read_saved_claude, ClaudeEndpoints, CLAUDE};
#[cfg(test)]
pub(crate) use codex::codex_card;
pub use codex::consume_codex_reset_credit;
pub(crate) use codex::{read_saved_codex, CodexEndpoints};
pub(crate) use reading::keep_remembered;
pub(crate) use snapshots::Remembered;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;

use crate::accounts::claude_store::{KeychainProbe, StorageDir};
use crate::accounts::model::Identity;
use crate::dto::{AgentId, LimitsAccountDto, LimitsStatus, ProviderLimitsDto, Reading};
use crate::http::HttpError;
use crate::paths;
use claude::ClaudeSources;
use pipeline::finish;
#[cfg(test)]
use pipeline::resolve;
use snapshots::SnapshotStore;

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

/// The card a read of the signed-in account `account_id` reporting `reading` becomes: the pipeline's
/// own card, so its windows come in the order every card lists them whatever order they are given
/// in. For tests elsewhere that must not hand a consumer an order no read produces.
#[cfg(test)]
pub(crate) fn signed_in_card(
    provider: AgentId,
    account_id: &str,
    reading: Reading,
) -> ProviderLimitsDto {
    finish(
        provider,
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

/// Everything `read_limits` needs that tests replace: where the homes and snapshots live, the clock,
/// and what the Claude read needs.
struct Sources<'a, P: Fn(&StorageDir) -> KeychainProbe> {
    home: &'a Path,
    now_ms: i64,
    claude: ClaudeSources<'a, P>,
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
    read_limits_at(agent, force, &home)
}

/// [`read_limits`] under `home`, with the live Claude login memo, Keychain probe, endpoints and
/// clock.
fn read_limits_at(agent: AgentId, force: bool, home: &Path) -> Vec<ProviderLimitsDto> {
    read_limits_in(
        agent,
        force,
        Sources {
            home,
            now_ms: Utc::now().timestamp_millis(),
            claude: ClaudeSources::live(),
        },
    )
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

fn read_limits_in<P: Fn(&StorageDir) -> KeychainProbe>(
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
        AgentId::Claude => claude::claude_current(force, sources),
        AgentId::Codex => codex::codex_limits(home, force),
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
/// reading, persisted; then the other remembered accounts, newest first, each flagged when the user
/// archived it. The flag goes on once the legacy history a card replaced is hidden, so an archived
/// card keeps hiding it; the signed-in card is never flagged, since being signed in unarchives it.
fn aggregate_accounts(store: &SnapshotStore, current: ProviderLimitsDto) -> Vec<ProviderLimitsDto> {
    let provider = current.provider;
    let current_account = current.account.as_ref().map(|account| account.id.clone());
    let mut remembered = store.load(provider);
    remembered.retain(|snapshot| {
        snapshot.account.as_ref().map(|account| &account.id) != current_account.as_ref()
    });
    let current = store.remember(current).card;
    let mut accounts =
        snapshots::without_superseded(std::iter::once(current).chain(remembered).collect());
    let archived = store.archived(provider);
    for card in &mut accounts {
        card.archived = !card.current_account
            && card
                .account
                .as_ref()
                .is_some_and(|account| archived.contains(&account.id));
    }
    accounts
}

/// `card`, keeping what its read could not tell from what `home` remembers of its account, written
/// over that account's file when it observed something datable ([`SnapshotStore::remember`]).
/// Called for a saved profile's poll and the first usage after a sign-in, only once the account
/// registry accepted the login it was read with.
pub(crate) fn remember(home: &Path, card: ProviderLimitsDto) -> Remembered {
    SnapshotStore::for_home(home).remember(card)
}

/// The ids of the accounts of `provider` the user archived under `home`.
pub(crate) fn archived(home: &Path, provider: AgentId) -> BTreeSet<String> {
    SnapshotStore::for_home(home).archived(provider)
}

/// Archives or unarchives the accounts `ids` of `agent` names, the user's own action from a Limits
/// card; whether the archive changed.
pub fn set_archived(agent: AgentId, ids: &[String], archived: bool) -> Result<bool, String> {
    let home = paths::user_home().map_err(|error| error.message)?;
    set_archived_at(&home, agent, ids, archived)
}

/// Archives or unarchives `ids` of `provider` under `home`, the user's own action; whether the
/// archive changed.
pub(crate) fn set_archived_at(
    home: &Path,
    provider: AgentId,
    ids: &[String],
    archived: bool,
) -> Result<bool, String> {
    SnapshotStore::for_home(home).set_archived(provider, ids, archived)
}

/// What `home` remembers for `provider`, as the next read loads it.
#[cfg(test)]
pub(crate) fn remembered(home: &Path, provider: AgentId) -> Vec<ProviderLimitsDto> {
    SnapshotStore::for_home(home).load(provider)
}

/// Why a saved profile's read gave no card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SavedReadError {
    /// The request failed. `HttpError::Unauthorized` is a login the service refused, or one that
    /// holds no access token.
    Http(HttpError),
    /// The login now signs in as a different account than the profile's.
    OtherAccount,
    /// The login's access token has expired, and on-n-off does not renew this login: the client
    /// it was saved from does, and the next time that login is captured its renewal comes along.
    Expired,
}

impl From<HttpError> for SavedReadError {
    fn from(error: HttpError) -> Self {
        Self::Http(error)
    }
}

/// Where a saved profile's read asks, for either provider: [`SavedReadUrls::LIVE`] in the app,
/// loopback servers in tests.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SavedReadUrls<'a> {
    pub(crate) claude: ClaudeEndpoints<'a>,
    pub(crate) codex: CodexEndpoints<'a>,
}

impl SavedReadUrls<'static> {
    /// The services a saved read asks in the app.
    pub(crate) const LIVE: Self = Self {
        claude: CLAUDE,
        codex: codex::CODEX,
    };
}

/// A saved profile's read as its card: known by the profile's observation key, never the signed-in
/// account's. A read that observed nothing is an error, so the card keeps what it remembers.
fn saved_card(
    identity: &Identity,
    mut parsed: Parsed,
) -> Result<ProviderLimitsDto, SavedReadError> {
    let label = parsed.account.and_then(|account| account.label);
    parsed.account = Some(scoped_account(identity, label));
    let mut dto = finish(identity.provider, LimitsStatus::Ok, None, parsed);
    dto.current_account = false;
    if !dto.reading.has_observations() {
        return Err(
            HttpError::Parse("Usage response contained no quota observations.".into()).into(),
        );
    }
    Ok(dto)
}

/// The account a card of `identity` is known by, labelled `label`: its observation key, with the
/// key its cards had before scoped identities as its legacy id (Claude's user, Codex's workspace),
/// through which it supersedes them (`snapshots::without_superseded`).
fn scoped_account(identity: &Identity, label: Option<String>) -> LimitsAccountDto {
    LimitsAccountDto {
        id: identity.observation_key(),
        label,
        legacy_id: Some(if identity.provider == AgentId::Codex {
            identity.workspace_id.clone()
        } else {
            identity.user_id.clone()
        }),
    }
}

pub(crate) fn clear_login_memo() {
    credentials::CLAUDE_LOGIN.clear();
}

#[cfg(test)]
mod tests;
