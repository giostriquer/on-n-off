use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use crate::dto::{AgentId, LimitsStatus, ProviderLimitsDto};

const MAX_FAILURE_BACKOFF: Duration = Duration::from_secs(60 * 60);

struct CachedRead {
    refreshed_at: Instant,
    entries: Vec<ProviderLimitsDto>,
    consecutive_failures: u32,
}

static CLAUDE_CACHE: Mutex<Option<CachedRead>> = Mutex::new(None);
static CODEX_CACHE: Mutex<Option<CachedRead>> = Mutex::new(None);
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
    let Some(cache) = cache_for(agent) else {
        return crate::limits::read_limits(agent, force);
    };
    let interval = poll_interval();
    read_through_cache(cache, interval, force, |force| {
        crate::limits::read_limits(agent, force)
    })
}

pub fn forget_snapshot(agent: AgentId, account_id: &str) -> Result<(), String> {
    let Some(cache) = cache_for(agent) else {
        return crate::limits::forget_snapshot(agent, account_id);
    };
    forget_through_cache(cache, account_id, || {
        crate::limits::forget_snapshot(agent, account_id)
    })
}

fn cache_for(agent: AgentId) -> Option<&'static Mutex<Option<CachedRead>>> {
    match agent {
        AgentId::Claude => Some(&CLAUDE_CACHE),
        AgentId::Codex => Some(&CODEX_CACHE),
        AgentId::Antigravity | AgentId::Cursor => None,
    }
}

fn read_through_cache<F>(
    cache: &Mutex<Option<CachedRead>>,
    interval: Duration,
    force: bool,
    read: F,
) -> Vec<ProviderLimitsDto>
where
    F: FnOnce(bool) -> Vec<ProviderLimitsDto>,
{
    let mut cached = cache
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
            return entry.entries.clone();
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
    entries
}

fn forget_through_cache<F>(
    cache: &Mutex<Option<CachedRead>>,
    account_id: &str,
    forget: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    let mut cached = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    forget()?;
    if let Some(entry) = cached.as_mut() {
        entry.entries.retain(|snapshot| {
            snapshot.current_account
                || snapshot
                    .account
                    .as_ref()
                    .is_none_or(|account| account.id != account_id)
        });
    }
    Ok(())
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
mod tests {
    use super::*;
    use crate::dto::LimitsAccountDto;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };
    use std::thread;
    use std::time::{Duration, Instant};

    fn snapshot(
        account_id: &str,
        current_account: bool,
        status: LimitsStatus,
    ) -> ProviderLimitsDto {
        ProviderLimitsDto {
            provider: AgentId::Claude,
            status,
            message: None,
            account: Some(LimitsAccountDto {
                id: account_id.into(),
                label: None,
            }),
            current_account,
            plan: None,
            windows: Vec::new(),
            credits: None,
        }
    }

    #[test]
    fn cache_freshness_uses_the_configured_interval_and_force_bypasses_it() {
        let refreshed_at = Instant::now();
        let interval = Duration::from_secs(5 * 60);

        assert!(cache_is_fresh(
            refreshed_at,
            refreshed_at + Duration::from_secs(299),
            interval,
            0,
            false
        ));
        assert!(!cache_is_fresh(
            refreshed_at,
            refreshed_at + interval,
            interval,
            0,
            false
        ));
        assert!(!cache_is_fresh(
            refreshed_at,
            refreshed_at + Duration::from_secs(1),
            interval,
            0,
            true
        ));
    }

    #[test]
    fn automatic_failures_back_off_for_every_consumer() {
        let refreshed_at = Instant::now();
        let interval = Duration::from_secs(5 * 60);

        assert!(cache_is_fresh(
            refreshed_at,
            refreshed_at + interval,
            interval,
            1,
            false
        ));
        assert!(!cache_is_fresh(
            refreshed_at,
            refreshed_at + interval * 2,
            interval,
            1,
            false
        ));
        assert!(current_read_failed(&[snapshot(
            "current",
            true,
            LimitsStatus::Failed
        )]));
        assert!(!current_read_failed(&[snapshot(
            "remembered",
            false,
            LimitsStatus::Failed
        )]));
        assert_eq!(
            next_failure_count(1, &[snapshot("current", true, LimitsStatus::Failed)]),
            2
        );
        assert_eq!(
            next_failure_count(2, &[snapshot("current", true, LimitsStatus::Ok)]),
            0
        );
    }

    #[test]
    fn shared_cache_coalesces_automatic_consumers_and_force_bypasses_it() {
        let cache = Arc::new(Mutex::new(None));
        let calls = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(Barrier::new(3));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let cache = cache.clone();
                let calls = calls.clone();
                let start = start.clone();
                thread::spawn(move || {
                    start.wait();
                    read_through_cache(&cache, Duration::from_secs(300), false, |_| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(25));
                        Vec::new()
                    })
                })
            })
            .collect();
        start.wait();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        read_through_cache(&cache, Duration::from_secs(300), true, |force| {
            assert!(force);
            calls.fetch_add(1, Ordering::SeqCst);
            Vec::new()
        });
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn forgetting_a_snapshot_removes_it_from_the_shared_cache_only_after_disk_success() {
        let cache = Mutex::new(Some(CachedRead {
            refreshed_at: Instant::now(),
            entries: vec![
                snapshot("current", true, LimitsStatus::Ok),
                snapshot("forgotten", false, LimitsStatus::Ok),
            ],
            consecutive_failures: 0,
        }));

        forget_through_cache(&cache, "forgotten", || Ok(())).unwrap();
        assert_eq!(cache.lock().unwrap().as_ref().unwrap().entries.len(), 1);

        let result = forget_through_cache(&cache, "current", || Err("disk failed".into()));
        assert_eq!(result, Err("disk failed".into()));
        assert_eq!(cache.lock().unwrap().as_ref().unwrap().entries.len(), 1);
    }
}
