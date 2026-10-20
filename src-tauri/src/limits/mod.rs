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
mod muse;
pub(crate) mod outcome;
mod pipeline;
mod plan;
mod reading;
mod renewal;
mod snapshots;
mod standing;
mod window;
pub(crate) mod window_at;

#[cfg(test)]
pub(crate) use codex::codex_card;
pub use codex::consume_codex_reset_credit;
pub(crate) use codex::{read_saved_codex, CodexEndpoints};
pub(crate) use muse::mint::{read_saved as read_saved_muse, verify as verify_saved_muse};
pub(crate) use outcome::{Accepted, NativeOutcome, ReadFailure};
pub(crate) use reading::keep_remembered;
pub(crate) use snapshots::{without_superseded, Remembered};
#[cfg(test)]
pub(crate) use standing::cases as standing_cases;
pub(crate) use window::{cycle_turned_over, Observation, ResetInstant};

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;

use crate::accounts::model::Identity;
#[cfg(test)]
use crate::dto::LimitsStatus;
use crate::dto::{AgentId, LimitsAccountDto, ProviderLimitsDto, Reading};
use crate::paths;
use crate::provider_catalogue::SessionRollouts;
use outcome::Read;
#[cfg(test)]
use pipeline::finish;
use snapshots::SnapshotStore;

const DEFAULT_ACCOUNT: &str = "default";
static CLAUDE_READ_LOCK: Mutex<()> = Mutex::new(());
static CODEX_READ_LOCK: Mutex<()> = Mutex::new(());
static MUSE_READ_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, PartialEq)]
struct Parsed {
    account: Option<LimitsAccountDto>,
    reading: Reading,
}

#[cfg(test)]
impl Parsed {
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

struct Sources<'a> {
    home: &'a Path,
    now_ms: i64,
}

pub(crate) struct NativeRead {
    pub(crate) cards: Vec<ProviderLimitsDto>,
    pub(crate) outcome: NativeOutcome,
}

pub(crate) fn read_limits(agent: AgentId, force: bool) -> NativeRead {
    let home = match paths::user_home() {
        Ok(home) => home,
        Err(error) => {
            let (card, outcome) = outcome::signed_in(
                agent,
                Read::anonymous(ReadFailure::Unavailable(format!(
                    "Could not read the stored login: {}",
                    error.message
                ))),
            );
            return NativeRead {
                cards: vec![card],
                outcome,
            };
        }
    };
    read_limits_at(agent, force, &home)
}

fn read_limits_at(agent: AgentId, force: bool, home: &Path) -> NativeRead {
    read_limits_in(
        agent,
        force,
        Sources {
            home,
            now_ms: Utc::now().timestamp_millis(),
        },
    )
}

pub(crate) fn native_login(agent: AgentId) -> Option<String> {
    let home = paths::user_home().ok()?;
    native_login_at(agent, &home)
}

fn native_login_at(agent: AgentId, home: &Path) -> Option<String> {
    (limits_reader(agent)?.login)(home).map(|account| account.id)
}

pub(crate) fn verify_signed_in(agent: AgentId, force: bool) -> Result<(), String> {
    let home = paths::user_home().map_err(|error| error.message)?;
    verify_signed_in_at(agent, force, &home)
}

fn verify_signed_in_at(agent: AgentId, force: bool, home: &Path) -> Result<(), String> {
    let failure = match limits_reader(agent) {
        Some(reader) => {
            let _provider_guard = provider_read_guard(agent);
            (reader.read)(home, force).answer.err()
        }
        None => Some(ReadFailure::Unsupported(outcome::Unsupported::NoLimits)),
    };
    outcome::verified(agent, failure.as_ref())
}

pub fn forget_snapshot(
    agent: AgentId,
    account_id: &str,
    expected_email: Option<&str>,
) -> Result<Vec<String>, String> {
    let home = paths::user_home().map_err(|error| error.message)?;
    if let Some(email) = expected_email {
        return SnapshotStore::for_home(&home).forget_matching_email(agent, account_id, email);
    }
    SnapshotStore::for_home(&home).forget(agent, account_id)
}

#[derive(Clone, Copy)]
struct Reader {
    read: fn(&Path, bool) -> Read,
    login: fn(&Path) -> Option<LimitsAccountDto>,
}

fn limits_reader(agent: AgentId) -> Option<Reader> {
    match agent {
        AgentId::Claude => Some(Reader {
            read: |home, _| claude::claude_current(home),
            login: claude::known_account,
        }),
        AgentId::Codex => Some(Reader {
            read: codex::codex_read,
            login: codex_app_server::known_account,
        }),
        AgentId::Muse => Some(Reader {
            read: |home, _| muse::muse_current(home),
            login: muse::known_account,
        }),
        AgentId::Antigravity | AgentId::Cursor => None,
    }
}

fn read_limits_in(agent: AgentId, force: bool, sources: Sources<'_>) -> NativeRead {
    let Some(reader) = limits_reader(agent) else {
        let (card, outcome) = outcome::signed_in(
            agent,
            Read::anonymous(ReadFailure::Unsupported(outcome::Unsupported::NoLimits)),
        );
        return NativeRead {
            cards: vec![card],
            outcome,
        };
    };
    let _provider_guard = provider_read_guard(agent);
    read_accounts(agent, sources, |home| (reader.read)(home, force))
}

fn read_accounts(
    agent: AgentId,
    sources: Sources<'_>,
    read: impl FnOnce(&Path) -> Read,
) -> NativeRead {
    let home = sources.home;
    let observed_at =
        chrono::DateTime::<Utc>::from_timestamp_millis(sources.now_ms).unwrap_or_else(Utc::now);
    let (current, outcome) = outcome::signed_in(agent, read(home));
    let store = SnapshotStore::for_home(home);
    let mut cards = aggregate_accounts(&store, current);
    match agent.limits_facts().session_rollouts {
        Some(SessionRollouts::Codex) => {
            let before = cards.clone();
            if codex_sessions::merge_recent(home, observed_at, &mut cards) > 0 {
                store.save_changed(&before, &cards);
            }
        }
        None => {}
    }
    NativeRead { cards, outcome }
}

fn provider_read_lock(agent: AgentId) -> &'static Mutex<()> {
    match agent {
        AgentId::Claude => &CLAUDE_READ_LOCK,
        AgentId::Codex => &CODEX_READ_LOCK,
        AgentId::Muse => &MUSE_READ_LOCK,
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

fn aggregate_accounts(store: &SnapshotStore, current: ProviderLimitsDto) -> Vec<ProviderLimitsDto> {
    let current_account = current.account.as_ref().map(|account| account.id.clone());
    let mut remembered = store.load(current.provider);
    remembered.retain(|snapshot| {
        snapshot.account.as_ref().map(|account| &account.id) != current_account.as_ref()
    });
    let current = store.remember(current).card;
    snapshots::without_superseded(std::iter::once(current).chain(remembered).collect())
}

pub(crate) fn remember(home: &Path, card: ProviderLimitsDto) -> Remembered {
    SnapshotStore::for_home(home).remember(card)
}

pub(crate) fn archived(home: &Path, provider: AgentId) -> BTreeSet<String> {
    SnapshotStore::for_home(home).archived(provider)
}

pub fn set_archived(agent: AgentId, ids: &[String], archived: bool) -> Result<bool, String> {
    let home = paths::user_home().map_err(|error| error.message)?;
    set_archived_at(&home, agent, ids, archived)
}

pub(crate) fn set_archived_at(
    home: &Path,
    provider: AgentId,
    ids: &[String],
    archived: bool,
) -> Result<bool, String> {
    SnapshotStore::for_home(home).set_archived(provider, ids, archived)
}

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

pub fn flag_archived(agent: AgentId, entries: &mut [ProviderLimitsDto]) {
    if let Ok(home) = paths::user_home() {
        flag_archived_at(&home, agent, entries);
    }
}

pub(crate) fn flag_archived_at(home: &Path, agent: AgentId, entries: &mut [ProviderLimitsDto]) {
    let archived = SnapshotStore::for_home(home).archived(agent);
    mark_archived(entries, |id, _| archived.contains(id));
}

pub fn unarchive_signed_in(agent: AgentId, entries: &[ProviderLimitsDto]) -> bool {
    paths::user_home().is_ok_and(|home| unarchive_signed_in_at(&home, agent, entries))
}

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

pub(crate) fn unarchive_profile(
    home: &Path,
    identity: &Identity,
    email: Option<String>,
) -> Result<bool, String> {
    SnapshotStore::for_home(home)
        .unarchive_account(identity.provider, &scoped_account(identity, email))
}

#[cfg(test)]
pub(crate) fn remembered(home: &Path, provider: AgentId) -> Vec<ProviderLimitsDto> {
    SnapshotStore::for_home(home).load(provider)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SavedReadUrls<'a> {
    pub(crate) codex: CodexEndpoints<'a>,
    pub(crate) muse: &'a str,
}

impl SavedReadUrls<'static> {
    pub(crate) const LIVE: Self = Self {
        codex: codex::CODEX,
        muse: muse::mint::KEY_ENDPOINT,
    };
}

pub(crate) fn scoped_account(identity: &Identity, label: Option<String>) -> LimitsAccountDto {
    LimitsAccountDto {
        id: identity.observation_key(),
        label,
        legacy_id: identity.legacy_key(),
    }
}

#[cfg(test)]
mod tests;
