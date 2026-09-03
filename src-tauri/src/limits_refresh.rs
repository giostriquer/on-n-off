use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use crate::dto::{AgentId, LimitsStatus, ProviderLimitsDto};
use crate::read_revision::{self, Reading, Revision, Source};

const MAX_FAILURE_BACKOFF: Duration = Duration::from_secs(60 * 60);

struct CachedRead {
    refreshed_at: Instant,
    entries: Vec<ProviderLimitsDto>,
    consecutive_failures: u32,
}

/// One provider's shared read, with the [`Revision`] its consumers watch to notice a refresh made
/// through another surface and the [`Source`] they are told about it under. The revision is
/// deliberately outside the mutex: a read holds that lock for the whole provider call, and asking
/// "is there anything newer?" must never block. Carrying the source here rather than mapping an
/// `AgentId` to one a second time is what keeps "has a shared cache" a single fact.
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

/// One process-wide provider read per configured interval. Every automatic consumer shares this
/// cache; a user-requested refresh bypasses it and replaces the cached observation.
pub fn read_limits(agent: AgentId, force: bool) -> Vec<ProviderLimitsDto> {
    read_limits_revisioned(agent, force).0
}

/// `read_limits` plus the shared cache's revision the entries were taken at, for a consumer that
/// caches the answer itself and needs to notice a refresh made through another consumer.
pub fn read_limits_revisioned(agent: AgentId, force: bool) -> (Vec<ProviderLimitsDto>, u64) {
    let Some(cache) = cache_for(agent) else {
        return (crate::limits::read_limits(agent, force), 0);
    };
    let interval = poll_interval();
    let (entries, reading) = read_through_cache(cache, interval, force, |force| {
        crate::limits::read_limits(agent, force)
    });
    announce(cache, reading);
    (entries, reading.revision())
}

/// Tells the windows about a replacement, outside the lock the read held, so neither a provider
/// call waits on the announcement nor the announcement on a provider call.
fn announce(cache: &Cache, reading: Reading) {
    if reading.replaced() {
        read_revision::announce(cache.source);
    }
}

/// The shared cache's current revision for `agent`, `0` for a provider that is not cached. Never
/// blocks: a caller polls this to decide whether a cheap cache-served read is worth making. Only
/// the native notch keeps its own copy of a read, so only macOS asks.
#[cfg(any(target_os = "macos", test))]
pub fn revision(agent: AgentId) -> u64 {
    cache_for(agent).map_or(0, |cache| cache.revision.current())
}

pub fn forget_snapshot(agent: AgentId, account_id: &str) -> Result<(), String> {
    let Some(cache) = cache_for(agent) else {
        return crate::limits::forget_snapshot(agent, account_id);
    };
    let reading = forget_through_cache(cache, account_id, || {
        crate::limits::forget_snapshot(agent, account_id)
    })?;
    // Dropping a remembered account replaces the shared entries like any read does, and the other
    // window lists those accounts too: without this it goes on offering to forget an account this
    // one already forgot, until its own poll comes round.
    announce(cache, reading);
    Ok(())
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
    // Every read is remembered, a failed one included, so the consumer that answers the
    // announcement is served this same entry rather than calling the provider again.
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

#[cfg(test)]
mod tests;
