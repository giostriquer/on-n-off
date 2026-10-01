use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use crate::dto::{AgentId, LimitsStatus, ProviderLimitsDto, ResetCreditOutcome};
use crate::read_revision::{self, Reading, Revision, Source};

const MAX_FAILURE_BACKOFF: Duration = Duration::from_secs(60 * 60);

struct CachedRead {
    refreshed_at: Instant,
    entries: Vec<ProviderLimitsDto>,
    consecutive_failures: u32,
}

struct Cache {
    read: Mutex<Option<CachedRead>>,
    revision: Revision,
    source: Source,
}

impl Cache {
    const fn new(source: Source) -> Self {
        Self {
            read: Mutex::new(None),
            revision: Revision::new(),
            source,
        }
    }
}

static CLAUDE_CACHE: Cache = Cache::new(Source::LimitsClaude);
static CODEX_CACHE: Cache = Cache::new(Source::LimitsCodex);
static POLL_SECONDS: AtomicU64 = AtomicU64::new(0);

pub fn set_poll_minutes(minutes: u16) {
    POLL_SECONDS.store(u64::from(minutes) * 60, Ordering::Release);
}

pub fn poll_interval() -> Duration {
    let cached = POLL_SECONDS.load(Ordering::Acquire);
    if cached > 0 {
        return Duration::from_secs(cached);
    }
    let minutes = crate::settings::load_settings().limits_poll_minutes;
    let seconds = u64::from(minutes) * 60;
    let _ = POLL_SECONDS.compare_exchange(0, seconds, Ordering::AcqRel, Ordering::Acquire);
    Duration::from_secs(POLL_SECONDS.load(Ordering::Acquire))
}

pub fn read_limits(agent: AgentId, force: bool) -> Vec<ProviderLimitsDto> {
    read_limits_revisioned(agent, force).0
}

pub fn read_limits_revisioned(agent: AgentId, force: bool) -> (Vec<ProviderLimitsDto>, u64) {
    let Some(cache) = cache_for(agent) else {
        return (crate::limits::read_limits(agent, force), 0);
    };
    let Some(_account_read) = crate::accounts::activity::read(agent) else {
        let read = cache
            .read
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        return (
            read.as_ref().map(|r| r.entries.clone()).unwrap_or_default(),
            cache.revision.current(),
        );
    };
    let interval = poll_interval();
    read_provider(
        cache,
        interval,
        force,
        &|force| crate::limits::read_limits(agent, force),
        &|entries| crate::limits::unarchive_signed_in(agent, entries),
        &|force, entries| crate::accounts::usage::refresh(agent, force, entries),
        &|entries| crate::limits::flag_archived(agent, entries),
    )
}

fn read_provider(
    cache: &Cache,
    interval: Duration,
    force: bool,
    native: &dyn Fn(bool) -> Vec<ProviderLimitsDto>,
    unarchive: &dyn Fn(&[ProviderLimitsDto]) -> bool,
    saved: &dyn Fn(bool, &mut Vec<ProviderLimitsDto>),
    flag: &dyn Fn(&mut [ProviderLimitsDto]),
) -> (Vec<ProviderLimitsDto>, u64) {
    let mut unarchived = false;
    let (entries, reading) = read_through_cache(cache, interval, force, |force| {
        let mut entries = native(force);
        unarchived = unarchive(&entries);
        saved(force, &mut entries);
        flag(&mut entries);
        entries
    });
    announce(cache, reading);
    if unarchived {
        read_revision::announce(Source::Accounts);
    }
    (entries, reading.revision())
}

fn announce(cache: &Cache, reading: Reading) {
    if reading.replaced() {
        read_revision::announce(cache.source);
    }
}

#[cfg(any(target_os = "macos", test))]
pub fn revision(agent: AgentId) -> u64 {
    cache_for(agent).map_or(0, |cache| cache.revision.current())
}

pub fn forget_snapshot(
    agent: AgentId,
    account_id: &str,
    expected_email: Option<&str>,
) -> Result<(), String> {
    let Some(cache) = cache_for(agent) else {
        return crate::limits::forget_snapshot(agent, account_id, expected_email);
    };
    let reading = forget_through_cache(cache, account_id, || {
        crate::limits::forget_snapshot(agent, account_id, expected_email)
    })?;
    announce(cache, reading);
    Ok(())
}

pub fn set_archived(agent: AgentId, ids: &[String], archived: bool) -> Result<(), String> {
    let Some(cache) = cache_for(agent) else {
        return Err(format!(
            "{} has no accounts to archive.",
            agent.display_name()
        ));
    };
    set_archived_with(
        cache,
        ids,
        archived,
        || crate::limits::set_archived(agent, ids, archived),
        |force| {
            let _ = read_limits(agent, force);
        },
    )
}

fn set_archived_with(
    cache: &Cache,
    ids: &[String],
    archived: bool,
    write: impl FnOnce() -> Result<bool, String>,
    reread: impl FnOnce(bool),
) -> Result<(), String> {
    if ids.is_empty() {
        return Err("No account to archive.".to_string());
    }
    let (reading, changed) = archive_through_cache(cache, ids, archived, write)?;
    announce(cache, reading);
    if changed {
        read_revision::announce(Source::Accounts);
    }
    if !archived {
        reread(true);
    }
    Ok(())
}

fn archive_through_cache<F>(
    cache: &Cache,
    ids: &[String],
    archived: bool,
    write: F,
) -> Result<(Reading, bool), String>
where
    F: FnOnce() -> Result<bool, String>,
{
    let mut cached = cache
        .read
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let changed = write()?;
    let edited = cached.as_mut().is_some_and(|read| {
        crate::limits::mark_archived(&mut read.entries, |id, was| {
            if ids.iter().any(|named| named == id) {
                archived
            } else {
                was
            }
        })
    });
    let reading = if edited {
        Reading::Replaced(cache.revision.bump())
    } else {
        Reading::Unchanged(cache.revision.current())
    };
    Ok((reading, changed))
}

pub fn consume_codex_reset_credit(
    account_id: &str,
    idempotency_key: &str,
    how: crate::limits::ResetSpend,
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
        AgentId::Antigravity | AgentId::Cursor => None,
    }
}

fn read_through_cache<F>(
    cache: &Cache,
    interval: Duration,
    force: bool,
    read: F,
) -> (Vec<ProviderLimitsDto>, Reading)
where
    F: FnOnce(bool) -> Vec<ProviderLimitsDto>,
{
    let mut cached = cache
        .read
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = Instant::now();
    if let Some(entry) = cached.as_ref() {
        if cache_is_fresh(
            entry.refreshed_at,
            now,
            interval,
            entry.consecutive_failures,
            force,
        ) {
            return (
                entry.entries.clone(),
                Reading::Unchanged(cache.revision.current()),
            );
        }
    }
    let previous_failures = cached
        .as_ref()
        .map_or(0, |entry| entry.consecutive_failures);
    let entries = read(force);
    let consecutive_failures = next_failure_count(previous_failures, &entries);
    *cached = Some(CachedRead {
        refreshed_at: Instant::now(),
        entries: entries.clone(),
        consecutive_failures,
    });
    (entries, Reading::Replaced(cache.revision.bump()))
}

fn forget_through_cache<F>(cache: &Cache, account_id: &str, forget: F) -> Result<Reading, String>
where
    F: FnOnce() -> Result<(), String>,
{
    let mut cached = cache
        .read
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    forget()?;
    let Some(entry) = cached.as_mut() else {
        return Ok(Reading::Unchanged(cache.revision.current()));
    };
    let before = entry.entries.len();
    entry.entries.retain(|snapshot| {
        snapshot.current_account
            || snapshot
                .account
                .as_ref()
                .is_none_or(|account| account.id != account_id)
    });
    if entry.entries.len() == before {
        return Ok(Reading::Unchanged(cache.revision.current()));
    }
    Ok(Reading::Replaced(cache.revision.bump()))
}

fn current_read_failed(entries: &[ProviderLimitsDto]) -> bool {
    entries
        .iter()
        .any(|entry| entry.current_account && entry.status == LimitsStatus::Failed)
}

fn next_failure_count(previous: u32, entries: &[ProviderLimitsDto]) -> u32 {
    if current_read_failed(entries) {
        previous.saturating_add(1)
    } else {
        0
    }
}

fn cache_is_fresh(
    refreshed_at: Instant,
    now: Instant,
    interval: Duration,
    consecutive_failures: u32,
    force: bool,
) -> bool {
    let interval = crate::monitor::backoff(interval, consecutive_failures, MAX_FAILURE_BACKOFF);
    !force && now.saturating_duration_since(refreshed_at) < interval
}

pub(crate) fn account_changed(agent: AgentId) {
    if let Some(cache) = cache_for(agent) {
        *cache
            .read
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
    let _ = read_limits(agent, false);
}

#[cfg(test)]
mod tests;
