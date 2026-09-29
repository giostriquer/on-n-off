//! Subscription rate limits aggregated per account from provider-owned clients/endpoints,
//! remembered snapshots, and account-correlated local observations. Claude Code reports every Claude
//! account's usage itself (`claude_cli.rs`), renewing its own login as it does, and on-n-off reads
//! no Claude credential here; Codex owns its authentication and refresh lifecycle through
//! app-server, which its reader (`codex.rs`) asks.
//!
//! `read_limits` never fails for provider-side reasons; every outcome is a `ProviderLimitsDto`
//! whose `status` + `message` tell the UI what to show. Because each CLI stores one login at a
//! time, successful reads are also remembered per account (numbers only) so accounts the user
//! has switched away from stay visible with each window's observation time.

mod backend_memo;
mod claude;
pub(crate) mod claude_cli;
mod claude_config;
mod codex;
mod codex_app_server;
mod codex_sessions;
pub(crate) mod credits_spent;
pub(crate) mod json;
pub(crate) mod login;
mod pipeline;
mod reading;
mod renewal;
mod snapshots;

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

use crate::accounts::model::Identity;
use crate::dto::{AgentId, LimitsAccountDto, LimitsStatus, ProviderLimitsDto, Reading};
use crate::http::HttpError;
use crate::paths;
use pipeline::finish;
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

/// Everything `read_limits` needs that tests replace: where the homes and snapshots live, and the
/// clock.
struct Sources<'a> {
    home: &'a Path,
    now_ms: i64,
}

/// Current subscription limits for one provider, followed by remembered observations for its other
/// accounts. Blocking: runs Claude Code's usage report for Claude or a bounded Codex app-server
/// process; call it off the UI thread. `force` requests fresh Codex authentication.
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

/// [`read_limits`] under `home`, with the live clock.
fn read_limits_at(agent: AgentId, force: bool, home: &Path) -> Vec<ProviderLimitsDto> {
    read_limits_in(
        agent,
        force,
        Sources {
            home,
            now_ms: Utc::now().timestamp_millis(),
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

fn read_limits_in(agent: AgentId, force: bool, sources: Sources<'_>) -> Vec<ProviderLimitsDto> {
    let _provider_guard = match agent {
        AgentId::Claude | AgentId::Codex => Some(provider_read_guard(agent)),
        AgentId::Antigravity | AgentId::Cursor => None,
    };
    let home = sources.home;
    let observed_at =
        chrono::DateTime::<Utc>::from_timestamp_millis(sources.now_ms).unwrap_or_else(Utc::now);
    let current = match agent {
        AgentId::Claude => claude::claude_current(home),
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
/// reading, persisted; then the other remembered accounts, newest first.
fn aggregate_accounts(store: &SnapshotStore, current: ProviderLimitsDto) -> Vec<ProviderLimitsDto> {
    let current_account = current.account.as_ref().map(|account| account.id.clone());
    let mut remembered = store.load(current.provider);
    remembered.retain(|snapshot| {
        snapshot.account.as_ref().map(|account| &account.id) != current_account.as_ref()
    });
    let current = store.remember(current).card;
    snapshots::without_superseded(std::iter::once(current).chain(remembered).collect())
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

/// Sets each card's `archived` to what `archived` says of its account id and its current flag: the
/// one rule every flag goes through. The signed-in card is never archived, since being signed in
/// unarchives an account, and a card that names no account is not one. Whether any flag changed.
pub(crate) fn mark_archived(
    entries: &mut [ProviderLimitsDto],
    archived: impl Fn(&str, bool) -> bool,
) -> bool {
    let mut changed = false;
    for card in entries {
        let flag = !card.current_account
            && card
                .account
                .as_ref()
                .is_some_and(|account| archived(&account.id, card.archived));
        changed |= flag != card.archived;
        card.archived = flag;
    }
    changed
}

/// Flags the cards of a read of `agent` whose accounts the user archived, once the read has named
/// its signed-in account and the saved polls have answered.
pub fn flag_archived(agent: AgentId, entries: &mut [ProviderLimitsDto]) {
    if let Ok(home) = paths::user_home() {
        flag_archived_at(&home, agent, entries);
    }
}

/// [`flag_archived`] under `home`.
pub(crate) fn flag_archived_at(home: &Path, agent: AgentId, entries: &mut [ProviderLimitsDto]) {
    let archived = SnapshotStore::for_home(home).archived(agent);
    mark_archived(entries, |id, _| archived.contains(id));
}

/// Unarchives the signed-in account a read of `agent` gave `entries` for, since being signed in
/// unarchives an account whether it was switched to in on-n-off or in the provider's CLI; whether
/// the archive changed.
pub fn unarchive_signed_in(agent: AgentId, entries: &[ProviderLimitsDto]) -> bool {
    paths::user_home().is_ok_and(|home| unarchive_signed_in_at(&home, agent, entries))
}

/// [`unarchive_signed_in`] under `home`: the signed-in card's id, and the legacy id its history was
/// kept under when that history is this account's (`SnapshotStore::unarchive_account`). A write
/// that fails changed nothing and leaves the account archived, for the next read to try again.
pub(crate) fn unarchive_signed_in_at(
    home: &Path,
    agent: AgentId,
    entries: &[ProviderLimitsDto],
) -> bool {
    entries
        .iter()
        .find(|entry| entry.current_account)
        .and_then(|entry| entry.account.as_ref())
        .is_some_and(|account| {
            SnapshotStore::for_home(home).unarchive_account(agent, account) == Ok(true)
        })
}

/// Unarchives the saved profile `identity`, labelled `email`, which the user just saved or signed in
/// to again (Save account, Add account, Sign in again), as those undo a Remove: its own card, and
/// its legacy history when that history names the same email. Automatic remembering never calls
/// this. Whether the archive changed.
pub(crate) fn unarchive_profile(
    home: &Path,
    identity: &Identity,
    email: Option<String>,
) -> Result<bool, String> {
    SnapshotStore::for_home(home)
        .unarchive_account(identity.provider, &scoped_account(identity, email))
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
    /// No reading could be had, for the reason the card shows as it is: the provider's client could
    /// not report or reported nothing, needs an update, or the login has not reached its home yet.
    Unavailable(&'static str),
}

impl From<HttpError> for SavedReadError {
    fn from(error: HttpError) -> Self {
        Self::Http(error)
    }
}

/// Where a saved profile's read asks: [`SavedReadUrls::LIVE`] in the app, loopback servers in
/// tests. Only Codex's is sent over HTTP; Claude Code reads a saved Claude account itself.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SavedReadUrls<'a> {
    pub(crate) codex: CodexEndpoints<'a>,
}

impl SavedReadUrls<'static> {
    /// The services a saved read asks in the app.
    pub(crate) const LIVE: Self = Self {
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

#[cfg(test)]
mod tests;
