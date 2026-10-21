use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use crate::accounts::{Roster, SignInDeadlines};
use crate::dto::{AgentId, ColumnEntryDto, LimitsStatus, ProviderLimitsDto, ResetCreditOutcome};
use crate::limits::{NativeOutcome, NativeRead, ReadFailure};
use crate::read_revision::{self, Reading, Revision, Source};

mod account_actions;

const MAX_FAILURE_BACKOFF: Duration = Duration::from_secs(60 * 60);
const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);

struct CachedRead {
    entries: Vec<ProviderLimitsDto>,
    saved_refreshed_at: Instant,
    native: NativeState,
}

struct NativeState {
    refreshed_at: Instant,
    consecutive_failures: u32,
    retry_floor: Duration,
    outcome: NativeOutcome,
}

impl NativeState {
    fn after(previous: Option<&Self>, outcome: NativeOutcome, refreshed_at: Instant) -> Self {
        let failures = previous.map_or(0, |previous| previous.consecutive_failures);
        Self {
            refreshed_at,
            consecutive_failures: if outcome.counts_toward_backoff() {
                failures.saturating_add(1)
            } else {
                0
            },
            retry_floor: outcome.retry_floor(chrono::Utc::now().timestamp()),
            outcome,
        }
    }

    fn holds_back(&self, now: Instant, login: &dyn Fn() -> Option<String>) -> bool {
        now.saturating_duration_since(self.refreshed_at) < self.retry_floor
            && login() == self.outcome.login
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Served {
    pub(crate) provider: AgentId,
    pub(crate) entries: Vec<ProviderLimitsDto>,
    pub(crate) failure: Option<ReadFailure>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Changed {
    limits: bool,
    accounts: bool,
}

impl Changed {
    fn without(self, announced: Self) -> Self {
        Self {
            limits: self.limits && !announced.limits,
            accounts: self.accounts && !announced.accounts,
        }
    }

    fn send(self, source: Source) {
        if self.limits {
            read_revision::announce(source);
        }
        if self.accounts {
            read_revision::announce(Source::Accounts);
        }
    }
}

pub(crate) struct ProviderRead {
    served: Served,
    revision: u64,
    announced: Changed,
}

struct Refresh<'a> {
    native: bool,
    saved: bool,
    previous: &'a [ProviderLimitsDto],
}

struct Listing {
    roster: Option<Roster>,
    rereads: u64,
    ends: SignInDeadlines,
}

struct Cache {
    provider: AgentId,
    read: Mutex<Option<CachedRead>>,
    listing: Mutex<Listing>,
    revision: Revision,
    source: Source,
}

impl Cache {
    const fn new(provider: AgentId, source: Source) -> Self {
        Self {
            provider,
            read: Mutex::new(None),
            listing: Mutex::new(Listing {
                roster: None,
                rereads: 0,
                ends: SignInDeadlines::new(),
            }),
            revision: Revision::new(),
            source,
        }
    }

    fn lock_read(&self) -> std::sync::MutexGuard<'_, Option<CachedRead>> {
        self.read
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn listing(&self) -> std::sync::MutexGuard<'_, Listing> {
        self.listing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn column(&self, entries: &[ProviderLimitsDto]) -> Vec<ColumnEntryDto> {
        let (roster, ends) = {
            let listing = self.listing();
            (
                listing.roster.clone().unwrap_or_default(),
                listing.ends.clone(),
            )
        };
        crate::limits::column(
            entries,
            &roster,
            &ends,
            chrono::Utc::now().timestamp_millis(),
        )
    }

    fn served_column(
        &self,
        entries: &[ProviderLimitsDto],
        home: Option<&std::path::Path>,
    ) -> Vec<ColumnEntryDto> {
        let mut column = self.column(entries);
        if let Some(home) = home {
            crate::limits::desktop_app::mark_desktop_account(self.provider, &mut column, home);
        }
        column
    }

    fn keep_listed(&self, roster: Roster, rereads_before: u64) {
        let mut listing = self.listing();
        if listing.rereads == rereads_before {
            listing.roster = Some(roster);
        }
    }

    fn served(&self, read: Option<&CachedRead>) -> Served {
        Served {
            provider: self.provider,
            entries: read.map(|read| read.entries.clone()).unwrap_or_default(),
            failure: read.and_then(|read| read.native.outcome.failure.clone()),
        }
    }
}

static CLAUDE_CACHE: Cache = Cache::new(AgentId::Claude, Source::LimitsClaude);
static CODEX_CACHE: Cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
static MUSE_CACHE: Cache = Cache::new(AgentId::Muse, Source::LimitsMuse);
pub(crate) static POLL_SECONDS: AtomicU64 = AtomicU64::new(0);

pub fn set_poll_minutes(minutes: u16) {
    POLL_SECONDS.store(u64::from(minutes) * 60, Ordering::Release);
}

pub fn poll_interval() -> Duration {
    poll_interval_in(&POLL_SECONDS)
}

pub(crate) fn poll_interval_in(memo: &AtomicU64) -> Duration {
    crate::settings::remembered_seconds(memo, || {
        u64::from(crate::settings::load_settings().limits_poll_minutes) * 60
    })
}

pub(crate) fn check_interval(default: Duration) -> Duration {
    default.min(CHECK_INTERVAL)
}

pub(crate) fn failure_backoff(interval: Duration, failures: u32) -> Duration {
    crate::monitor::backoff(interval, failures, MAX_FAILURE_BACKOFF)
}

fn active_interval(entries: &[ProviderLimitsDto], default: Duration) -> Duration {
    let used = entries
        .iter()
        .find(|entry| entry.current_account && !entry.archived && entry.status == LimitsStatus::Ok)
        .filter(|entry| entry.account.is_some())
        .into_iter()
        .flat_map(|entry| &entry.reading.windows)
        .map(|window| window.used_percent)
        .filter(|used| used.is_finite())
        .reduce(f64::max);
    if used.is_some_and(|used| (90.0..100.0).contains(&used)) {
        check_interval(default)
    } else {
        default
    }
}

pub fn read_limits(agent: AgentId, force: bool) -> Vec<ProviderLimitsDto> {
    read_served(agent, force).0.entries
}

pub fn read_column(agent: AgentId, force: bool) -> Vec<ColumnEntryDto> {
    let entries = read_served(agent, force).0.entries;
    match cache_for(agent) {
        Some(cache) => cache.served_column(&entries, crate::paths::user_home().ok().as_deref()),
        None => crate::limits::column(
            &entries,
            &Roster::default(),
            &SignInDeadlines::new(),
            chrono::Utc::now().timestamp_millis(),
        ),
    }
}

pub fn read_limits_revisioned(agent: AgentId, force: bool) -> (Vec<ProviderLimitsDto>, u64) {
    let (served, revision) = read_served(agent, force);
    (served.entries, revision)
}

pub(crate) fn read_served(agent: AgentId, force: bool) -> (Served, u64) {
    let read = read_announced(agent, force);
    (read.served, read.revision)
}

fn read_announced(agent: AgentId, force: bool) -> ProviderRead {
    let Some(cache) = cache_for(agent) else {
        let read = crate::limits::read_limits(agent, force);
        let served = Served {
            provider: agent,
            entries: read.cards,
            failure: read.outcome.failure,
        };
        return ProviderRead {
            served,
            revision: 0,
            announced: Changed::default(),
        };
    };
    let Some(_account_read) = crate::accounts::activity::read(agent) else {
        return ProviderRead {
            served: cache.served(cache.lock_read().as_ref()),
            revision: cache.revision.current(),
            announced: Changed::default(),
        };
    };
    let interval = poll_interval();
    read_provider(
        cache,
        interval,
        force,
        &Native {
            read: &|force| crate::limits::read_limits(agent, force),
            login: &|| crate::limits::native_login(agent),
        },
        &|entries| crate::limits::unarchive_signed_in(agent, entries),
        &|force, entries| crate::accounts::usage::refresh_and_list(agent, force, entries),
        &|entries| crate::limits::flag_archived(agent, entries),
    )
}

struct Native<'a> {
    read: &'a dyn Fn(bool) -> NativeRead,
    login: &'a dyn Fn() -> Option<String>,
}

fn read_provider(
    cache: &Cache,
    interval: Duration,
    force: bool,
    native: &Native<'_>,
    unarchive: &dyn Fn(&[ProviderLimitsDto]) -> bool,
    refresh_and_list: &dyn Fn(bool, &mut Vec<ProviderLimitsDto>) -> Option<Roster>,
    flag: &dyn Fn(&mut [ProviderLimitsDto]),
) -> ProviderRead {
    let mut unarchived = false;
    let (served, reading) =
        read_through_cache(cache, interval, force, native.login, |force, refresh| {
            let (mut cards, outcome) = if refresh.native {
                let NativeRead { mut cards, outcome } = (native.read)(force);
                unarchived = unarchive(&cards);
                if !refresh.saved {
                    cards = keep_saved_cards(cards, refresh.previous);
                }
                (cards, Some(outcome))
            } else {
                (refresh.previous.to_vec(), None)
            };
            if refresh.saved {
                let rereads = cache.listing().rereads;
                if let Some(roster) = refresh_and_list(force, &mut cards) {
                    cache.keep_listed(roster, rereads);
                }
            }
            flag(&mut cards);
            (cards, outcome)
        });
    let announced = Changed {
        limits: reading.replaced(),
        accounts: unarchived,
    };
    announced.send(cache.source);
    ProviderRead {
        served,
        revision: reading.revision(),
        announced,
    }
}

fn keep_saved_cards(
    mut entries: Vec<ProviderLimitsDto>,
    previous: &[ProviderLimitsDto],
) -> Vec<ProviderLimitsDto> {
    for card in previous.iter().filter(|card| !card.current_account) {
        let Some(account) = &card.account else {
            continue;
        };
        match entries.iter_mut().find(|entry| {
            entry
                .account
                .as_ref()
                .is_some_and(|other| other.id == account.id)
        }) {
            Some(entry) if !entry.current_account => entry.clone_from(card),
            Some(_) => {}
            None => entries.push(card.clone()),
        }
    }
    entries
}

fn announce(cache: &Cache, reading: Reading) {
    if reading.replaced() {
        read_revision::announce(cache.source);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub fn revision(agent: AgentId) -> u64 {
    cache_for(agent).map_or(0, |cache| cache.revision.current())
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
pub(crate) fn bump_revision(agent: AgentId) -> u64 {
    cache_for(agent).map_or(0, |cache| cache.revision.bump())
}

pub fn remove_account(agent: AgentId, account_id: &str) -> Result<(), String> {
    let (cache, stores) = account_stores(agent)?;
    account_actions::remove_with(cache, account_id, &stores)
}

pub fn set_account_archived(
    agent: AgentId,
    account_id: &str,
    archived: bool,
) -> Result<(), String> {
    let (cache, stores) = account_stores(agent)?;
    account_actions::archive_with(cache, account_id, archived, &stores)
}

fn account_stores(agent: AgentId) -> Result<(&'static Cache, account_actions::AtHome), String> {
    let cache = cache_for(agent)
        .ok_or_else(|| format!("{} has no Limits accounts.", agent.display_name()))?;
    let home = crate::paths::user_home().map_err(|error| error.message)?;
    Ok((
        cache,
        account_actions::AtHome {
            home,
            agent,
            read: |agent, force| read_announced(agent, force).announced,
        },
    ))
}

pub fn forget_snapshot(
    agent: AgentId,
    account_id: &str,
    expected_email: Option<&str>,
) -> Result<Vec<String>, String> {
    let forget = || crate::limits::forget_snapshot(agent, account_id, expected_email);
    match cache_for(agent) {
        Some(cache) => account_actions::forget_with(cache, forget),
        None => forget(),
    }
}

pub fn set_archived(agent: AgentId, ids: &[String], archived: bool) -> Result<(), String> {
    let (cache, stores) = account_stores(agent)?;
    account_actions::archive_ids(cache, archived, &stores, |_| Ok(ids.to_vec()))
}

pub(crate) fn roster_changed(agent: AgentId) {
    let (Some(cache), Ok(home)) = (cache_for(agent), crate::paths::user_home()) else {
        return;
    };
    announce(cache, reread_roster(cache, || Roster::read(&home, agent)));
}

fn reread_roster(cache: &Cache, read: impl FnOnce() -> Option<Roster>) -> Reading {
    relist(cache, None, read)
}

pub(crate) fn accounts_listed(agent: AgentId, ends: SignInDeadlines) {
    let (Some(cache), Ok(home)) = (cache_for(agent), crate::paths::user_home()) else {
        return;
    };
    announce(
        cache,
        relist(cache, Some(ends), || Roster::read(&home, agent)),
    );
}

fn relist(
    cache: &Cache,
    ends: Option<SignInDeadlines>,
    read: impl FnOnce() -> Option<Roster>,
) -> Reading {
    if relisted(cache, ends, read) {
        Reading::Replaced(cache.revision.bump())
    } else {
        Reading::Unchanged(cache.revision.current())
    }
}

fn relisted(
    cache: &Cache,
    ends: Option<SignInDeadlines>,
    read: impl FnOnce() -> Option<Roster>,
) -> bool {
    let roster = read();
    let mut listing = cache.listing();
    let mut changed = false;
    if let Some(ends) = ends {
        changed |= listing.ends != ends;
        listing.ends = ends;
    }
    if let Some(roster) = roster {
        listing.rereads += 1;
        changed |= listing.roster.as_ref() != Some(&roster);
        listing.roster = Some(roster);
    }
    changed
}

pub fn consume_codex_reset_credit(
    account_id: &str,
    idempotency_key: &str,
    how: crate::reset_alert::ResetSpend,
) -> Result<ResetCreditOutcome, String> {
    spend_then_refresh(
        idempotency_key,
        || crate::accounts::activity::read(AgentId::Codex),
        || crate::limits::consume_codex_reset_credit(account_id, idempotency_key, how),
        || {
            let _ = read_limits(AgentId::Codex, true);
        },
    )
}

fn spend_then_refresh<L, O>(
    idempotency_key: &str,
    lease: impl FnOnce() -> Option<L>,
    spend: impl FnOnce() -> Result<O, String>,
    refresh: impl FnOnce(),
) -> Result<O, String> {
    if idempotency_key.trim().is_empty() || idempotency_key.len() > 128 {
        return Err("Invalid banked reset attempt.".to_string());
    }
    let spent = {
        let Some(_lease) = lease() else {
            return Err(
                "A Codex account change is running. Try again when it finishes.".to_string(),
            );
        };
        spend()
    };
    refresh();
    spent
}

fn cache_for(agent: AgentId) -> Option<&'static Cache> {
    match agent {
        AgentId::Claude => Some(&CLAUDE_CACHE),
        AgentId::Codex => Some(&CODEX_CACHE),
        AgentId::Muse => Some(&MUSE_CACHE),
        AgentId::Antigravity | AgentId::Cursor => None,
    }
}

fn read_through_cache<F>(
    cache: &Cache,
    interval: Duration,
    force: bool,
    native_login: &dyn Fn() -> Option<String>,
    read: F,
) -> (Served, Reading)
where
    F: FnOnce(bool, Refresh<'_>) -> (Vec<ProviderLimitsDto>, Option<NativeOutcome>),
{
    let mut cached = cache.lock_read();
    let now = Instant::now();
    let native = cached.as_ref().is_none_or(|entry| {
        !entry.native.holds_back(now, native_login)
            && !cache_is_fresh(
                entry.native.refreshed_at,
                now,
                active_interval(&entry.entries, interval),
                entry.native.consecutive_failures,
                force,
            )
    });
    let saved = cached
        .as_ref()
        .is_none_or(|entry| !cache_is_fresh(entry.saved_refreshed_at, now, interval, 0, force));
    if !native && !saved {
        return (
            cache.served(cached.as_ref()),
            Reading::Unchanged(cache.revision.current()),
        );
    }
    let (entries, outcome) = read(
        force,
        Refresh {
            native,
            saved,
            previous: cached.as_ref().map_or(&[], |entry| &entry.entries),
        },
    );
    let finished_at = Instant::now();
    let previous = cached.take();
    let saved_refreshed_at = match &previous {
        Some(previous) if !saved => previous.saved_refreshed_at,
        _ => finished_at,
    };
    let native_state = match (outcome, previous) {
        (Some(outcome), previous) if native => NativeState::after(
            previous.as_ref().map(|previous| &previous.native),
            outcome,
            finished_at,
        ),
        (None, Some(previous)) if !native => previous.native,
        _ => {
            unreachable!("a read reports a native outcome exactly when it read the native account")
        }
    };
    *cached = Some(CachedRead {
        entries,
        saved_refreshed_at,
        native: native_state,
    });
    (
        cache.served(cached.as_ref()),
        Reading::Replaced(cache.revision.bump()),
    )
}

fn cache_is_fresh(
    refreshed_at: Instant,
    now: Instant,
    interval: Duration,
    consecutive_failures: u32,
    force: bool,
) -> bool {
    let interval = failure_backoff(interval, consecutive_failures);
    !force && now.saturating_duration_since(refreshed_at) < interval
}

pub(crate) fn account_changed(agent: AgentId) {
    let read = || {
        let _ = read_limits(agent, false);
    };
    match (cache_for(agent), crate::paths::user_home()) {
        (Some(cache), Ok(home)) => {
            account_changed_in(cache, || Roster::read(&home, agent), read);
        }
        (Some(cache), Err(_)) => account_changed_in(cache, || None, read),
        (None, _) => read(),
    }
}

fn account_changed_in(cache: &Cache, relist: impl FnOnce() -> Option<Roster>, read: impl FnOnce()) {
    *cache.lock_read() = None;
    announce(cache, reread_roster(cache, relist));
    read();
}

#[cfg(test)]
mod tests;
