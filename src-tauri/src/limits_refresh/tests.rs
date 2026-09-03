use super::*;
use crate::dto::LimitsAccountDto;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Barrier,
};
use std::thread;
use std::time::{Duration, Instant};

fn snapshot(account_id: &str, current_account: bool, status: LimitsStatus) -> ProviderLimitsDto {
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
