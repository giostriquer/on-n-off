use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Barrier,
};
use std::thread;
use std::time::{Duration, Instant};

fn snapshot(account_id: &str, current_account: bool, status: LimitsStatus) -> ProviderLimitsDto {
    ProviderLimitsDto {
        status,
        current_account,
        ..ProviderLimitsDto::for_test(AgentId::Claude, account_id)
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

/// Every automatic read paces itself by this interval. A test build has no user home, so it is
/// the settings default, never whatever a developer's own settings file says.
#[test]
fn a_test_builds_poll_interval_is_the_settings_default() {
    let minutes = crate::settings::AppSettings::default().limits_poll_minutes;
    assert_eq!(
        poll_interval(),
        Duration::from_secs(u64::from(minutes) * 60)
    );
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
    let cache = Arc::new(Cache::new(Source::LimitsClaude));
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
                .1
            })
        })
        .collect();
    start.wait();
    let seen: Vec<Reading> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        seen.contains(&Reading::Replaced(1)) && seen.contains(&Reading::Unchanged(1)),
        "one reader fetched and announces; the coalesced one is served the same revision and \
         announces nothing: {seen:?}"
    );

    read_through_cache(&cache, Duration::from_secs(300), true, |force| {
        assert!(force);
        calls.fetch_add(1, Ordering::SeqCst);
        Vec::new()
    });
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn only_a_read_that_replaces_the_cache_is_announced() {
    let cache = Cache::new(Source::LimitsClaude);
    let interval = Duration::from_secs(5 * 60);

    let (_, first) = read_through_cache(&cache, interval, false, |_| {
        vec![snapshot("current", true, LimitsStatus::Failed)]
    });
    assert_eq!(first, Reading::Replaced(1));

    let (_, unchanged) = read_through_cache(&cache, interval, false, |_| {
        unreachable!("the cached read is still fresh")
    });
    assert_eq!(
        unchanged,
        Reading::Unchanged(1),
        "the read a consumer makes in answer to an announcement is served the same entry and \
         announces nothing, which is what ends the exchange"
    );

    let (refreshed, forced) = read_through_cache(&cache, interval, true, |force| {
        assert!(force);
        vec![snapshot("current", true, LimitsStatus::Ok)]
    });
    assert_eq!(
        forced,
        Reading::Replaced(2),
        "a user refresh replaced the cached read, so every other surface is told"
    );

    let (served, seen) = read_through_cache(&cache, interval, false, |_| {
        unreachable!("the refreshed read is fresh")
    });
    assert_eq!(seen, Reading::Unchanged(2));
    assert_eq!(
        served, refreshed,
        "the next automatic consumer is served the refresh without a provider call"
    );
    assert_eq!(
        revision(AgentId::Cursor),
        0,
        "uncached providers never move"
    );
}

#[test]
fn a_failed_read_is_remembered_so_answering_its_announcement_costs_no_provider_call() {
    let cache = Cache::new(Source::LimitsCodex);
    let interval = Duration::from_secs(5 * 60);

    let (_, failed) = read_through_cache(&cache, interval, true, |_| {
        vec![snapshot("current", true, LimitsStatus::Failed)]
    });
    assert!(
        failed.replaced(),
        "unlike the GitHub reader, a failure here is remembered, so it is worth announcing"
    );

    let (served, reading) = read_through_cache(&cache, interval, false, |_| {
        unreachable!("a consumer answering the announcement must not reach the provider")
    });
    assert_eq!(reading, Reading::Unchanged(failed.revision()));
    assert_eq!(served[0].status, LimitsStatus::Failed);
}

#[test]
fn forgetting_a_snapshot_removes_it_from_the_shared_cache_only_after_disk_success() {
    let cache = Cache::new(Source::LimitsClaude);
    *cache.read.lock().unwrap() = Some(CachedRead {
        refreshed_at: Instant::now(),
        entries: vec![
            snapshot("current", true, LimitsStatus::Ok),
            snapshot("forgotten", false, LimitsStatus::Ok),
        ],
        consecutive_failures: 0,
    });

    let dropped = forget_through_cache(&cache, "forgotten", || Ok(())).unwrap();
    assert_eq!(
        cache.read.lock().unwrap().as_ref().unwrap().entries.len(),
        1
    );
    assert_eq!(
        dropped,
        Reading::Replaced(1),
        "the other window lists remembered accounts too, so it has to be told this one is gone"
    );

    let already_gone = forget_through_cache(&cache, "forgotten", || Ok(())).unwrap();
    assert_eq!(
        already_gone,
        Reading::Unchanged(1),
        "nothing was removed, so nobody is sent to re-read"
    );

    let result = forget_through_cache(&cache, "current", || Err("disk failed".into()));
    assert_eq!(result, Err("disk failed".into()));
    assert_eq!(
        cache.read.lock().unwrap().as_ref().unwrap().entries.len(),
        1
    );
}

fn seeded(entries: Vec<ProviderLimitsDto>) -> Cache {
    let cache = Cache::new(Source::LimitsCodex);
    *cache.read.lock().unwrap() = Some(CachedRead {
        refreshed_at: Instant::now(),
        entries,
        consecutive_failures: 0,
    });
    cache
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

/// Whether each cached entry is archived, in order.
fn flags(cache: &Cache) -> Vec<bool> {
    let read = cache.read.lock().unwrap();
    read.as_ref()
        .unwrap()
        .entries
        .iter()
        .map(|entry| entry.archived)
        .collect()
}

/// The shared entries follow the archive once the disk write succeeded, and only the entries it
/// names; the signed-in card is never flagged. Only an edit that changed an entry replaces them.
#[test]
fn archiving_flags_the_shared_entries_after_the_disk_write_and_never_the_signed_in_card() {
    let cache = seeded(vec![
        snapshot("signed-in", true, LimitsStatus::Ok),
        snapshot("saved", false, LimitsStatus::Ok),
        snapshot("legacy", false, LimitsStatus::Ok),
    ]);

    let archived = archive_through_cache(
        &cache,
        &ids(&["signed-in", "saved", "legacy"]),
        true,
        || {
            assert!(
                cache.read.try_lock().is_err(),
                "written under the cache lock"
            );
            Ok(true)
        },
    );
    assert_eq!(archived, Ok((Reading::Replaced(1), true)));
    assert_eq!(flags(&cache), [false, true, true]);

    let again = archive_through_cache(&cache, &ids(&["saved"]), true, || Ok(false));
    assert_eq!(again, Ok((Reading::Unchanged(1), false)));

    let unarchived = archive_through_cache(&cache, &ids(&["legacy"]), false, || Ok(true));
    assert_eq!(unarchived, Ok((Reading::Replaced(2), true)));
    assert_eq!(flags(&cache), [false, true, false]);

    let failed =
        archive_through_cache(
            &cache,
            &ids(&["legacy"]),
            true,
            || Err("disk failed".into()),
        );
    assert_eq!(failed, Err("disk failed".into()));
    assert_eq!(flags(&cache), [false, true, false], "nothing changed");
}

/// Archiving tells the other window its entries changed and the account list that the archive did;
/// unarchiving then reads the provider again, once the lock is released, so the account comes back
/// polled rather than remembered. A change that changed nothing is announced to nobody.
#[test]
fn archiving_is_announced_and_unarchiving_then_reads_the_provider_again() {
    let cache = seeded(vec![
        snapshot("signed-in", true, LimitsStatus::Ok),
        snapshot("saved", false, LimitsStatus::Ok),
    ]);
    let _ = read_revision::take_announced();
    let rereads = std::cell::Cell::new(0);
    let reread = |force: bool| {
        assert!(cache.read.try_lock().is_ok(), "read again outside the lock");
        assert!(
            force,
            "forced, so a held-back poll cannot leave it remembered"
        );
        rereads.set(rereads.get() + 1);
    };

    set_archived_with(&cache, &ids(&["saved"]), true, || Ok(true), reread).unwrap();
    assert_eq!(
        read_revision::take_announced(),
        [Source::LimitsCodex, Source::Accounts]
    );
    assert_eq!(rereads.get(), 0, "archiving reads nothing");

    set_archived_with(&cache, &ids(&["saved"]), true, || Ok(false), reread).unwrap();
    assert!(read_revision::take_announced().is_empty());

    set_archived_with(&cache, &ids(&["saved"]), false, || Ok(true), reread).unwrap();
    assert_eq!(
        read_revision::take_announced(),
        [Source::LimitsCodex, Source::Accounts]
    );
    assert_eq!(rereads.get(), 1);

    let refused = set_archived_with(&cache, &[], true, || Ok(true), reread);
    assert!(refused.is_err(), "no account named");
    let failed = set_archived_with(
        &cache,
        &ids(&["saved"]),
        false,
        || Err("disk".into()),
        reread,
    );
    assert_eq!(failed, Err("disk".into()));
    assert_eq!(rereads.get(), 1, "a failed unarchive reads nothing");
    assert!(read_revision::take_announced().is_empty());
}

/// A read does the archive's part in one place and one order, under the cache lock as Forget
/// writes: the signed-in account unarchived first, since being signed in unarchives an account, then
/// the saved accounts polled, then every card flagged once, so no flagged signed-in card is ever
/// cached and every saved card is flagged. The account list is announced outside the lock, only
/// when the read unarchived something: a replacement, never a read, so a read served from the cache
/// polls, writes and announces nothing.
#[test]
fn a_replacing_read_unarchives_polls_the_saved_accounts_then_flags_under_the_lock_and_announces_only_a_change(
) {
    let cache = Cache::new(Source::LimitsClaude);
    let _ = read_revision::take_announced();
    let log = std::cell::RefCell::new(Vec::new());
    let changes = std::cell::Cell::new(true);
    let native = |_| {
        log.borrow_mut().push("native");
        vec![
            snapshot("signed-in", true, LimitsStatus::Ok),
            snapshot("put-away", false, LimitsStatus::Ok),
        ]
    };
    let unarchive = |_: &[ProviderLimitsDto]| {
        assert!(
            cache.read.try_lock().is_err(),
            "unarchived under the cache lock"
        );
        log.borrow_mut().push("unarchive");
        changes.get()
    };
    let saved = |force, entries: &mut Vec<ProviderLimitsDto>| {
        assert!(
            cache.read.try_lock().is_err(),
            "polled under the cache lock"
        );
        log.borrow_mut()
            .push(if force { "saved, forced" } else { "saved" });
        entries.push(snapshot("saved", false, LimitsStatus::Ok));
    };
    let flag = |entries: &mut [ProviderLimitsDto]| {
        assert!(
            cache.read.try_lock().is_err(),
            "flagged under the cache lock"
        );
        log.borrow_mut().push("flag");
        for entry in entries.iter_mut().filter(|entry| !entry.current_account) {
            entry.archived = true;
        }
    };
    let interval = Duration::from_secs(300);
    let read = |force| read_provider(&cache, interval, force, &native, &unarchive, &saved, &flag).0;

    let entries = read(false);
    assert_eq!(*log.borrow(), ["native", "unarchive", "saved", "flag"]);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.archived)
            .collect::<Vec<_>>(),
        [false, true, true]
    );
    assert_eq!(
        read_revision::take_announced(),
        [Source::LimitsClaude, Source::Accounts]
    );

    assert_eq!(read(false), entries, "the cache holds the flagged cards");
    assert_eq!(
        log.borrow().len(),
        4,
        "a cached answer polls and writes nothing"
    );
    assert!(read_revision::take_announced().is_empty());

    changes.set(false);
    read(true);
    assert_eq!(
        log.borrow()[4..],
        ["native", "unarchive", "saved, forced", "flag"]
    );
    assert_eq!(
        read_revision::take_announced(),
        [Source::LimitsClaude],
        "nothing was unarchived, so the account list is not announced"
    );
}

struct Lease<'a>(&'a std::cell::RefCell<Vec<&'static str>>);

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.0.borrow_mut().push("lease released");
    }
}

#[test]
fn a_spend_is_followed_by_one_refresh_after_the_lease_is_released_even_when_it_fails() {
    for spent in [Ok("reset"), Err("Codex app-server timed out.".to_string())] {
        let log = std::cell::RefCell::new(Vec::new());
        let result = spend_then_refresh(
            "attempt-1",
            || {
                log.borrow_mut().push("lease taken");
                Some(Lease(&log))
            },
            || {
                log.borrow_mut().push("spend");
                spent.clone()
            },
            || log.borrow_mut().push("refresh"),
        );

        assert_eq!(result, spent);
        assert_eq!(
            log.into_inner(),
            ["lease taken", "spend", "lease released", "refresh"]
        );
    }
}

#[test]
fn nothing_is_spent_or_refreshed_during_an_account_change_or_for_an_invalid_attempt() {
    let log = std::cell::RefCell::new(Vec::new());
    let refused = spend_then_refresh(
        "attempt-1",
        || None::<()>,
        || -> Result<(), String> {
            log.borrow_mut().push("spend");
            Ok(())
        },
        || log.borrow_mut().push("refresh"),
    );
    assert!(refused.unwrap_err().contains("account change"));

    for key in ["", "   ", &"k".repeat(129)] {
        let invalid = spend_then_refresh(
            key,
            || {
                log.borrow_mut().push("lease taken");
                Some(())
            },
            || -> Result<(), String> {
                log.borrow_mut().push("spend");
                Ok(())
            },
            || log.borrow_mut().push("refresh"),
        );
        assert!(invalid.is_err());
    }
    assert!(log.into_inner().is_empty());
}
