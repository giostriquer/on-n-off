use super::*;
use std::cell::Cell;

fn quota(used: f64, current: bool) -> ProviderLimitsDto {
    let mut card = snapshot(
        if current { "active" } else { "saved" },
        current,
        LimitsStatus::Ok,
    );
    card.reading.windows.push(crate::dto::LimitWindowDto {
        id: "weekly".into(),
        label: "Weekly".into(),
        kind: crate::dto::LimitWindowKind::Weekly,
        used_percent: used,
        resets_at: None,
        window_seconds: None,
        observed_at: "2026-10-01T12:00:00Z".into(),
    });
    card
}

fn age_native(cache: &Cache, seconds: u64) {
    cache.read.lock().unwrap().as_mut().unwrap().refreshed_at =
        Instant::now() - Duration::from_secs(seconds);
}

#[test]
fn only_the_active_account_in_the_last_ten_percent_is_read_after_five_minutes() {
    for (used, current, status, expected_reads) in [
        (89.99, true, LimitsStatus::Ok, 1),
        (90.0, true, LimitsStatus::Ok, 2),
        (99.99, true, LimitsStatus::Ok, 2),
        (100.0, true, LimitsStatus::Ok, 1),
        (101.0, true, LimitsStatus::Ok, 1),
        (95.0, false, LimitsStatus::Ok, 1),
        (95.0, true, LimitsStatus::Failed, 1),
        (f64::NAN, true, LimitsStatus::Ok, 1),
    ] {
        let cache = Cache::new(Source::LimitsClaude);
        let native_reads = Cell::new(0);
        let saved_reads = Cell::new(0);
        let native = |_| {
            native_reads.set(native_reads.get() + 1);
            let mut card = quota(used, current);
            card.status = status;
            vec![card]
        };
        let saved = |_, _: &mut Vec<ProviderLimitsDto>| saved_reads.set(saved_reads.get() + 1);
        let read = || {
            read_provider(
                &cache,
                Duration::from_secs(1800),
                false,
                &native,
                &|_| false,
                &saved,
                &|_| {},
            )
        };
        read();
        age_native(&cache, 299);
        read();
        assert_eq!(native_reads.get(), 1, "read before five minutes at {used}%");
        age_native(&cache, 300);
        read();
        assert_eq!(
            native_reads.get(),
            expected_reads,
            "{used}% {current} {status:?}"
        );
        assert_eq!(saved_reads.get(), 1, "saved accounts were polled early");
    }
}

#[test]
fn reaching_a_hundred_percent_restores_the_default_and_keeps_saved_cards() {
    let cache = Cache::new(Source::LimitsCodex);
    let used = Cell::new(95.0);
    let reads = Cell::new(0);
    let saved_reads = Cell::new(0);
    let native = |_| {
        reads.set(reads.get() + 1);
        let mut entries = vec![quota(used.get(), true)];
        if reads.get() > 1 {
            entries.push(quota(40.0, false));
        }
        entries
    };
    let saved_card = {
        let mut card = quota(98.0, false);
        card.saved_profile = true;
        card.status = LimitsStatus::Failed;
        card.message = Some("Last reading retained".into());
        card
    };
    let saved = |_, entries: &mut Vec<ProviderLimitsDto>| {
        saved_reads.set(saved_reads.get() + 1);
        entries.push(saved_card.clone());
    };
    let read = || {
        read_provider(
            &cache,
            Duration::from_secs(1800),
            false,
            &native,
            &|_| false,
            &saved,
            &|_| {},
        )
    };
    read();
    age_native(&cache, 300);
    used.set(100.0);
    let (entries, revision) = read();
    assert_eq!(reads.get(), 2);
    assert_eq!(saved_reads.get(), 1);
    assert_eq!(entries, [quota(100.0, true), saved_card.clone()]);
    assert_eq!(revision, 2);
    age_native(&cache, 300);
    assert_eq!(
        read().1,
        2,
        "exhausted usage must wait for the default interval"
    );
    assert_eq!(reads.get(), 2);
    age_native(&cache, 1800);
    read();
    assert_eq!(reads.get(), 3);
}

#[test]
fn saved_deadlines_survive_extra_native_reads_and_do_not_reset_the_native_deadline() {
    let cache = Cache::new(Source::LimitsClaude);
    let used = Cell::new(94.0);
    let native_reads = Cell::new(0);
    let saved_reads = Cell::new(0);
    let native = |_| {
        native_reads.set(native_reads.get() + 1);
        vec![quota(used.get(), true)]
    };
    let saved = |_, _: &mut Vec<ProviderLimitsDto>| saved_reads.set(saved_reads.get() + 1);
    let read = |force| {
        read_provider(
            &cache,
            Duration::from_secs(1800),
            force,
            &native,
            &|_| false,
            &saved,
            &|_| {},
        )
    };
    read(false);
    let saved_at = cache
        .read
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .saved_refreshed_at;
    for value in [96.0, 99.0, 100.0] {
        age_native(&cache, 300);
        used.set(value);
        read(false);
        assert_eq!(
            cache
                .read
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .saved_refreshed_at,
            saved_at
        );
    }
    assert_eq!((native_reads.get(), saved_reads.get()), (4, 1));

    cache
        .read
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .saved_refreshed_at = Instant::now() - Duration::from_secs(1800);
    let native_at = cache.read.lock().unwrap().as_ref().unwrap().refreshed_at;
    read(false);
    assert_eq!((native_reads.get(), saved_reads.get()), (4, 2));
    assert_eq!(
        cache.read.lock().unwrap().as_ref().unwrap().refreshed_at,
        native_at
    );

    read(true);
    assert_eq!(
        (native_reads.get(), saved_reads.get()),
        (5, 3),
        "manual refresh reads both"
    );
}

#[test]
fn a_failed_native_read_backs_off_without_holding_back_saved_accounts() {
    let cache = Cache::new(Source::LimitsClaude);
    let native_reads = Cell::new(0);
    let saved_reads = Cell::new(0);
    let native = |_| {
        native_reads.set(native_reads.get() + 1);
        let mut card = quota(95.0, true);
        if native_reads.get() > 1 {
            card.status = LimitsStatus::Failed;
        }
        vec![card]
    };
    let saved = |_, _: &mut Vec<ProviderLimitsDto>| saved_reads.set(saved_reads.get() + 1);
    let read = || {
        read_provider(
            &cache,
            Duration::from_secs(900),
            false,
            &native,
            &|_| false,
            &saved,
            &|_| {},
        )
    };
    read();
    age_native(&cache, 300);
    read();
    assert_eq!((native_reads.get(), saved_reads.get()), (2, 1));
    age_native(&cache, 900);
    cache
        .read
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .saved_refreshed_at = Instant::now() - Duration::from_secs(900);
    read();
    assert_eq!((native_reads.get(), saved_reads.get()), (2, 2));
    assert_eq!(
        cache
            .read
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .consecutive_failures,
        1
    );
    age_native(&cache, 1800);
    read();
    assert_eq!(native_reads.get(), 3);
}

#[test]
fn the_fullest_window_controls_acceleration_and_a_reset_restores_the_default() {
    let cache = Cache::new(Source::LimitsClaude);
    let weekly = Cell::new(40.0);
    let session = Cell::new(95.0);
    let calls = Cell::new(0);
    let native = |_| {
        calls.set(calls.get() + 1);
        let mut card = quota(weekly.get(), true);
        let mut window = card.reading.windows[0].clone();
        window.id = "session".into();
        window.kind = crate::dto::LimitWindowKind::Session;
        window.used_percent = session.get();
        card.reading.windows.push(window);
        vec![card]
    };
    let read = |force| {
        read_provider(
            &cache,
            Duration::from_secs(1800),
            force,
            &native,
            &|_| false,
            &|_, _| {},
            &|_| {},
        )
    };
    read(false);
    age_native(&cache, 300);
    session.set(0.0);
    read(false);
    assert_eq!(
        calls.get(),
        2,
        "the session window can trigger a faster read"
    );
    age_native(&cache, 300);
    read(false);
    assert_eq!(calls.get(), 2, "a reset returns to the default interval");
    weekly.set(100.0);
    session.set(95.0);
    read(true);
    age_native(&cache, 300);
    read(false);
    assert_eq!(
        calls.get(),
        3,
        "an exhausted account must not keep the faster cadence"
    );
}

#[test]
fn an_external_switch_does_not_replace_the_new_active_card_with_its_saved_card() {
    let cache = Cache::new(Source::LimitsClaude);
    let switched = Cell::new(false);
    let native = |_| {
        if switched.get() {
            let mut card = quota(20.0, false);
            card.current_account = true;
            vec![card]
        } else {
            vec![quota(95.0, true)]
        }
    };
    let saved = |_, entries: &mut Vec<ProviderLimitsDto>| entries.push(quota(98.0, false));
    let read = || {
        read_provider(
            &cache,
            Duration::from_secs(1800),
            false,
            &native,
            &|_| false,
            &saved,
            &|_| {},
        )
    };
    read();
    switched.set(true);
    age_native(&cache, 300);
    let (entries, revision) = read();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].current_account);
    assert_eq!(entries[0].reading.windows[0].used_percent, 20.0);
    age_native(&cache, 300);
    assert_eq!(
        read().1,
        revision,
        "the new account uses its own usage to schedule reads"
    );
}

#[test]
fn an_extra_native_read_does_not_resurrect_a_superseded_legacy_card() {
    let mut legacy = quota(98.0, false);
    legacy.account.as_mut().unwrap().id = "team-b".into();
    legacy.account.as_mut().unwrap().label = Some("you@example.com".into());
    let cache = seeded(vec![quota(95.0, true), legacy]);
    let mut scoped = quota(20.0, true);
    scoped.account = Some(crate::dto::LimitsAccountDto {
        id: "profile:account-b".into(),
        legacy_id: Some("team-b".into()),
        label: Some("you@example.com".into()),
    });
    age_native(&cache, 300);

    let (entries, _) = read_provider(
        &cache,
        Duration::from_secs(1800),
        false,
        &|_| vec![scoped.clone()],
        &|_| false,
        &|_, _| panic!("saved accounts are not due"),
        &|_| {},
    );

    assert_eq!(entries, [scoped]);
}
