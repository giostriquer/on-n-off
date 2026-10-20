use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Barrier,
};
use std::thread;
use std::time::{Duration, Instant};

mod adaptive;

fn why() -> ReadFailure {
    ReadFailure::Unavailable("The source said why.".into())
}

fn native_read(cards: Vec<ProviderLimitsDto>) -> NativeRead {
    let failed = cards
        .iter()
        .any(|card| card.current_account && card.status == LimitsStatus::Failed);
    NativeRead {
        cards,
        outcome: NativeOutcome {
            failure: failed.then(why),
            login: None,
        },
    }
}

fn native(cards: Vec<ProviderLimitsDto>) -> (Vec<ProviderLimitsDto>, Option<NativeOutcome>) {
    let NativeRead { cards, outcome } = native_read(cards);
    (cards, Some(outcome))
}

fn no_login() -> Option<String> {
    None
}

fn cached(entries: Vec<ProviderLimitsDto>) -> CachedRead {
    CachedRead {
        entries,
        saved_refreshed_at: Instant::now(),
        native: NativeState {
            refreshed_at: Instant::now(),
            consecutive_failures: 0,
            retry_floor: Duration::ZERO,
            outcome: NativeOutcome::default(),
        },
    }
}

fn snapshot(account_id: &str, current_account: bool, status: LimitsStatus) -> ProviderLimitsDto {
    ProviderLimitsDto {
        status,
        current_account,
        ..ProviderLimitsDto::for_test(AgentId::Claude, account_id)
    }
}

#[test]
fn every_provider_with_a_limits_reader_has_its_own_cache_and_no_other_has_one() {
    for id in AgentId::ALL {
        assert_eq!(
            cache_for(id).is_some(),
            id.facts().limits.is_some(),
            "{id:?}"
        );
    }
    let sources: Vec<Source> = crate::provider_catalogue::WITH_LIMITS
        .into_iter()
        .filter_map(cache_for)
        .map(|cache| cache.source)
        .collect();
    for (i, source) in sources.iter().enumerate() {
        assert!(!sources[i + 1..].contains(source), "{source:?} is shared");
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
    let after = |failures, failure: Option<ReadFailure>| {
        let previous = NativeState {
            consecutive_failures: failures,
            ..cached(Vec::new()).native
        };
        NativeState::after(
            Some(&previous),
            NativeOutcome {
                failure,
                login: None,
            },
            refreshed_at,
        )
        .consecutive_failures
    };
    assert_eq!(after(1, Some(why())), 2);
    assert_eq!(after(2, None), 0);
    assert_eq!(after(2, Some(ReadFailure::SignedOut)), 0);
}

#[test]
fn a_failing_limits_read_waits_twice_as_long_each_time_up_to_sixteen_intervals_and_an_hour() {
    let minute = Duration::from_secs(60);
    for (failures, intervals) in [
        (0, 1),
        (1, 2),
        (2, 4),
        (3, 8),
        (4, 16),
        (5, 16),
        (9, 16),
        (u32::MAX, 16),
    ] {
        assert_eq!(
            failure_backoff(minute, failures),
            minute * intervals,
            "{failures}"
        );
    }
    for (interval, failures) in [(300, 5), (1000, 3), (7200, 1), (7200, 0)] {
        assert_eq!(
            failure_backoff(Duration::from_secs(interval), failures),
            Duration::from_secs(3600),
            "{failures} failures at {interval} s"
        );
    }
}

#[test]
fn a_native_read_that_keeps_failing_waits_the_limits_failure_backoff() {
    let refreshed_at = Instant::now();
    let second = Duration::from_secs(1);
    for interval in [Duration::from_secs(60), Duration::from_secs(300)] {
        for failures in [0, 1, 2, 3, 5] {
            let wait = failure_backoff(interval, failures);
            assert!(
                cache_is_fresh(
                    refreshed_at,
                    refreshed_at + wait - second,
                    interval,
                    failures,
                    false
                ),
                "{failures} failures at {interval:?}"
            );
            assert!(
                !cache_is_fresh(refreshed_at, refreshed_at + wait, interval, failures, false),
                "{failures} failures at {interval:?}"
            );
        }
    }
}

fn rate_limited(login: &str) -> NativeOutcome {
    NativeOutcome {
        failure: Some(ReadFailure::RateLimited(
            crate::http::RateLimitReset::RetryAfter(7200),
        )),
        login: Some(login.to_string()),
    }
}

#[test]
fn a_rate_limited_native_read_waits_out_its_retry_after_even_when_forced() {
    use std::cell::Cell;
    let cache = Cache::new(AgentId::Muse, Source::LimitsMuse);
    let interval = Duration::from_secs(300);
    let reads = Cell::new(0);
    let login = || Some("login-a".to_string());
    let read = |force| {
        read_through_cache(&cache, interval, force, &login, |_, refresh| {
            if refresh.native {
                reads.set(reads.get() + 1);
            }
            (
                vec![snapshot("current", true, LimitsStatus::Failed)],
                refresh.native.then(|| rate_limited("login-a")),
            )
        })
    };
    let age = |seconds| {
        cache
            .read
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .native
            .refreshed_at = Instant::now() - Duration::from_secs(seconds);
    };

    read(false);
    age(failure_backoff(interval, 1).as_secs() + 1);
    read(false);
    read(true);
    assert_eq!(
        reads.get(),
        1,
        "read the provider again inside the Retry-After"
    );
    age(7200);
    read(false);
    assert_eq!(
        reads.get(),
        2,
        "the read waits no longer than its Retry-After"
    );
}

#[test]
fn a_retry_after_holds_only_the_login_it_was_answered_for() {
    use std::cell::Cell;
    let cache = Cache::new(AgentId::Muse, Source::LimitsMuse);
    let interval = Duration::from_secs(300);
    let reads = Cell::new(0);
    let read = |login: &str| {
        let login = login.to_string();
        read_through_cache(
            &cache,
            interval,
            false,
            &|| Some(login.clone()),
            |_, refresh| {
                if refresh.native {
                    reads.set(reads.get() + 1);
                }
                (
                    vec![snapshot("current", true, LimitsStatus::Failed)],
                    refresh.native.then(|| rate_limited("login-a")),
                )
            },
        )
    };

    read("login-a");
    cache
        .read
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .native
        .refreshed_at = Instant::now() - failure_backoff(interval, 1) - Duration::from_secs(1);
    read("login-a");
    assert_eq!(reads.get(), 1, "the login it was answered for waits");
    read("login-b");
    assert_eq!(reads.get(), 2, "another login is read without waiting");
}

#[test]
fn a_provider_read_holds_a_forced_read_for_the_rate_limited_login_and_reads_another() {
    use std::cell::{Cell, RefCell};
    let cache = Cache::new(AgentId::Muse, Source::LimitsMuse);
    let login = RefCell::new("login-a");
    let reads = Cell::new(0);
    let native = |_| {
        reads.set(reads.get() + 1);
        NativeRead {
            cards: vec![snapshot("current", true, LimitsStatus::Failed)],
            outcome: rate_limited("login-a"),
        }
    };
    let current_login = || Some(login.borrow().to_string());
    let read = |force| {
        let native = Native {
            read: &native,
            login: &current_login,
        };
        read_provider(
            &cache,
            Duration::from_secs(300),
            force,
            &native,
            &|_| false,
            &|_, _| {},
            &|_| {},
        )
    };

    read(false);
    read(true);
    assert_eq!(reads.get(), 1, "a forced read waits for the same login");
    *login.borrow_mut() = "login-b";
    read(true);
    assert_eq!(reads.get(), 2, "another login is read at once");
}

#[test]
fn the_native_failure_is_served_with_the_cards_it_was_read_with() {
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    let interval = Duration::from_secs(300);
    let signed_out = || NativeOutcome {
        failure: Some(ReadFailure::SignedOut),
        login: None,
    };
    let cards = || vec![snapshot("current", true, LimitsStatus::SignedOut)];

    let (first, _) = read_through_cache(&cache, interval, false, &no_login, |_, _| {
        (cards(), Some(signed_out()))
    });
    let (served, _) = read_through_cache(&cache, interval, false, &no_login, |_, _| {
        unreachable!("the cached read is still fresh")
    });
    cache
        .read
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .saved_refreshed_at = Instant::now() - interval;
    let (saved_only, _) = read_through_cache(&cache, interval, false, &no_login, |_, refresh| {
        assert!(!refresh.native);
        (cards(), None)
    });

    for read in [first, served, saved_only] {
        assert_eq!(read.provider, AgentId::Codex);
        assert_eq!(read.failure, Some(ReadFailure::SignedOut));
        assert_eq!(read.entries, cards());
    }
}

#[test]
fn shared_cache_coalesces_automatic_consumers_and_force_bypasses_it() {
    let cache = Arc::new(Cache::new(AgentId::Claude, Source::LimitsClaude));
    let calls = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let cache = cache.clone();
            let calls = calls.clone();
            let start = start.clone();
            thread::spawn(move || {
                start.wait();
                read_through_cache(
                    &cache,
                    Duration::from_secs(300),
                    false,
                    &no_login,
                    |_, _| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(25));
                        native(Vec::new())
                    },
                )
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

    read_through_cache(
        &cache,
        Duration::from_secs(300),
        true,
        &no_login,
        |force, _| {
            assert!(force);
            calls.fetch_add(1, Ordering::SeqCst);
            native(Vec::new())
        },
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn only_a_read_that_replaces_the_cache_is_announced() {
    let cache = Cache::new(AgentId::Claude, Source::LimitsClaude);
    let interval = Duration::from_secs(5 * 60);

    let (_, first) = read_through_cache(&cache, interval, false, &no_login, |_, _| {
        native(vec![snapshot("current", true, LimitsStatus::Failed)])
    });
    assert_eq!(first, Reading::Replaced(1));

    let (_, unchanged) = read_through_cache(&cache, interval, false, &no_login, |_, _| {
        unreachable!("the cached read is still fresh")
    });
    assert_eq!(
        unchanged,
        Reading::Unchanged(1),
        "the read a consumer makes in answer to an announcement is served the same entry and \
         announces nothing, which is what ends the exchange"
    );

    let (refreshed, forced) = read_through_cache(&cache, interval, true, &no_login, |force, _| {
        assert!(force);
        native(vec![snapshot("current", true, LimitsStatus::Ok)])
    });
    assert_eq!(
        forced,
        Reading::Replaced(2),
        "a user refresh replaced the cached read, so every other surface is told"
    );

    let (served, seen) = read_through_cache(&cache, interval, false, &no_login, |_, _| {
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
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    let interval = Duration::from_secs(5 * 60);

    let (_, failed) = read_through_cache(&cache, interval, true, &no_login, |_, _| {
        native(vec![snapshot("current", true, LimitsStatus::Failed)])
    });
    assert!(
        failed.replaced(),
        "unlike the GitHub reader, a failure here is remembered, so it is worth announcing"
    );

    let (served, reading) = read_through_cache(&cache, interval, false, &no_login, |_, _| {
        unreachable!("a consumer answering the announcement must not reach the provider")
    });
    assert_eq!(reading, Reading::Unchanged(failed.revision()));
    assert_eq!(served.entries[0].status, LimitsStatus::Failed);
}

#[test]
fn forgetting_a_snapshot_removes_it_from_the_shared_cache_only_after_disk_success() {
    let cache = Cache::new(AgentId::Claude, Source::LimitsClaude);
    *cache.read.lock().unwrap() = Some(cached(vec![
        snapshot("current", true, LimitsStatus::Ok),
        snapshot("forgotten", false, LimitsStatus::Ok),
    ]));

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
    let cache = Cache::new(AgentId::Codex, Source::LimitsCodex);
    *cache.read.lock().unwrap() = Some(cached(entries));
    cache
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn flags(cache: &Cache) -> Vec<bool> {
    let read = cache.read.lock().unwrap();
    read.as_ref()
        .unwrap()
        .entries
        .iter()
        .map(|entry| entry.archived)
        .collect()
}

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

#[test]
fn a_replacing_read_unarchives_polls_the_saved_accounts_then_flags_under_the_lock_and_announces_only_a_change(
) {
    let cache = Cache::new(AgentId::Claude, Source::LimitsClaude);
    let _ = read_revision::take_announced();
    let log = std::cell::RefCell::new(Vec::new());
    let changes = std::cell::Cell::new(true);
    let native = |_| {
        log.borrow_mut().push("native");
        native_read(vec![
            snapshot("signed-in", true, LimitsStatus::Ok),
            snapshot("put-away", false, LimitsStatus::Ok),
        ])
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
    let read = |force| {
        let native = Native {
            read: &native,
            login: &no_login,
        };
        read_provider(&cache, interval, force, &native, &unarchive, &saved, &flag)
            .0
            .entries
    };

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
