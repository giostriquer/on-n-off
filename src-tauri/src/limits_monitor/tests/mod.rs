use super::*;
use crate::dto::{
    AgentId, LimitWindowDto, LimitWindowKind, LimitsStatus, ProviderLimitsDto, Reading,
};
use crate::paths::scratch_dir;
use std::fs;

fn snapshot(
    provider: AgentId,
    account_id: &str,
    account_label: &str,
    used_percent: f64,
    resets_at: Option<&str>,
) -> ProviderLimitsDto {
    ProviderLimitsDto::for_test(provider, account_id)
        .labelled(account_label)
        .with_reading(Reading {
            plan: Some("pro".into()),
            windows: vec![LimitWindowDto {
                id: "weekly".into(),
                label: "Weekly · all models".into(),
                kind: LimitWindowKind::Weekly,
                used_percent,
                resets_at: resets_at.map(str::to_string),
                window_seconds: Some(7 * 24 * 60 * 60),
                observed_at: "2026-08-19T12:00:00Z".into(),
                role: None,
                starts_on_first_message: false,
            }],
            ..Reading::default()
        })
}

fn observed_at(mut snapshot: ProviderLimitsDto, value: &str) -> ProviderLimitsDto {
    snapshot.reading.windows[0].observed_at = value.to_string();
    snapshot
}

#[test]
fn a_model_limit_crossing_one_hundred_percent_notifies_once() {
    let mut state = MonitorState::default();
    let mut before = snapshot(
        AgentId::Claude,
        "account-a",
        "me@example.com",
        99.0,
        Some("2026-08-24T12:00:00Z"),
    );
    before.reading.windows[0].id = "weekly_fable".into();
    before.reading.windows[0].label = "Weekly · Fable".into();
    before.reading.windows[0].kind = LimitWindowKind::Model;
    let mut exhausted = before.clone();
    exhausted.reading.windows[0].used_percent = 100.0;
    exhausted.reading.windows[0].observed_at = "2026-08-19T13:00:00Z".into();
    assert!(observe(&mut state, &[before]).is_empty());

    let events = observe(&mut state, std::slice::from_ref(&exhausted));

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, LimitEventKind::Exhausted);
    assert_eq!(events[0].provider, AgentId::Claude);
    assert_eq!(events[0].window_label, "Weekly · Fable");
    assert_eq!(events[0].previous_used_percent, 99.0);
    assert_eq!(events[0].used_percent, 100.0);
    assert!(observe(&mut state, &[exhausted]).is_empty());
}

#[test]
fn a_hidden_codex_window_reaching_its_limit_never_notifies() {
    let read = |weekly: u32, spark: u32, observed_at: &str| {
        let main = serde_json::json!({"limitId": "codex",
            "primary": {"usedPercent": weekly, "windowDurationMins": 10080, "resetsAt": 1787838960}});
        crate::limits::codex_card(
            serde_json::json!({
                "rateLimits": main,
                "rateLimitsByLimitId": {
                    "codex": main,
                    "codex_bengalfox": {"limitId": "codex_bengalfox",
                        "limitName": "GPT-5.3-Codex-Spark",
                        "primary": {"usedPercent": spark, "windowDurationMins": 10080,
                            "resetsAt": 1787859937}}
                }
            }),
            "account-a",
            observed_at,
        )
    };
    let mut state = MonitorState::default();
    assert!(observe(&mut state, &[read(50, 90, "2026-08-19T12:00:00Z")]).is_empty());

    let events = observe(&mut state, &[read(100, 100, "2026-08-19T13:00:00Z")]);

    let reached: Vec<(LimitEventKind, &str)> = events
        .iter()
        .map(|event| (event.kind, event.window_label.as_str()))
        .collect();
    assert_eq!(
        reached,
        [(LimitEventKind::Exhausted, "Weekly · all models")]
    );
}

#[test]
fn a_usage_correction_does_not_rearm_an_exhausted_limit() {
    let mut state = MonitorState::default();
    let below_limit = snapshot(
        AgentId::Claude,
        "account-a",
        "me@example.com",
        99.0,
        Some("2026-08-24T12:00:00Z"),
    );
    let at_limit = observed_at(
        snapshot(
            AgentId::Claude,
            "account-a",
            "me@example.com",
            100.0,
            Some("2026-08-24T12:00:00Z"),
        ),
        "2026-08-19T13:00:00Z",
    );
    assert!(observe(&mut state, std::slice::from_ref(&below_limit)).is_empty());
    assert_eq!(
        observe(&mut state, std::slice::from_ref(&at_limit)).len(),
        1
    );

    assert!(observe(&mut state, &[below_limit]).is_empty());
    assert!(observe(&mut state, &[at_limit]).is_empty());
}

#[test]
fn an_exhausted_limit_notification_is_not_described_as_a_reset() {
    let event = LimitEvent {
        kind: LimitEventKind::Exhausted,
        provider: AgentId::Claude,
        account_label: Some("me@example.com".into()),
        window_label: "Weekly · Fable".into(),
        previous_used_percent: 99.0,
        used_percent: 100.0,
    };

    let (title, body) = notification_copy(&event);

    assert!(title.contains("reached"));
    assert!(!title.contains("reset"));
    assert!(body.contains("Weekly · Fable"));
    assert!(body.contains("100%"));
    assert!(body.contains("me@example.com"));
}

#[test]
fn a_notification_words_its_percents_as_the_cards_and_the_notch_do() {
    let reset = LimitEvent {
        kind: LimitEventKind::Reset,
        provider: AgentId::Codex,
        account_label: None,
        window_label: "Weekly · all models".into(),
        previous_used_percent: 99.6,
        used_percent: 0.4,
    };

    let (_, body) = notification_copy(&reset);

    assert_eq!(body, "Weekly · all models reset from 99% to <1% used");
}

#[test]
fn an_exhausted_first_observation_is_only_a_baseline() {
    let mut state = MonitorState::default();

    let events = observe(
        &mut state,
        &[snapshot(
            AgentId::Codex,
            "account-a",
            "me@example.com",
            100.0,
            Some("2026-08-24T12:00:00Z"),
        )],
    );

    assert!(events.is_empty());
}

#[test]
fn a_reset_rearms_the_exhausted_notification() {
    let mut state = MonitorState::default();
    let at_limit = snapshot(
        AgentId::Codex,
        "account-a",
        "me@example.com",
        100.0,
        Some("2026-08-24T12:00:00Z"),
    );
    let reset = observed_at(
        snapshot(
            AgentId::Codex,
            "account-a",
            "me@example.com",
            0.0,
            Some("2026-08-31T12:00:00Z"),
        ),
        "2026-08-19T13:00:00Z",
    );
    let exhausted_again = observed_at(
        snapshot(
            AgentId::Codex,
            "account-a",
            "me@example.com",
            100.0,
            Some("2026-08-31T12:00:00Z"),
        ),
        "2026-08-19T14:00:00Z",
    );
    assert!(observe(&mut state, std::slice::from_ref(&at_limit)).is_empty());
    assert_eq!(observe(&mut state, &[reset]).len(), 1);

    let events = observe(&mut state, &[exhausted_again]);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].previous_used_percent, 0.0);
    assert_eq!(events[0].used_percent, 100.0);
}

#[test]
fn an_idle_window_read_after_its_reset_rearms_the_exhausted_notification() {
    let mut state = MonitorState::default();
    let muse = |used_percent, resets_at, at| {
        observed_at(
            snapshot(
                AgentId::Muse,
                "profile:you",
                "you@example.com",
                used_percent,
                resets_at,
            ),
            at,
        )
    };
    assert!(observe(
        &mut state,
        &[muse(
            100.0,
            Some("2026-10-16T21:45:00Z"),
            "2026-10-16T21:40:00Z"
        )]
    )
    .is_empty());

    let idle = observe(&mut state, &[muse(0.0, None, "2026-10-17T09:00:00Z")]);

    assert_eq!(idle.len(), 1);
    assert_eq!(idle[0].kind, LimitEventKind::Reset);
    assert_eq!(idle[0].previous_used_percent, 100.0);
    assert_eq!(idle[0].used_percent, 0.0);
    assert!(observe(
        &mut state,
        &[muse(
            40.0,
            Some("2026-10-18T12:00:00Z"),
            "2026-10-18T08:00:00Z"
        )]
    )
    .is_empty());
    let exhausted_again = observe(
        &mut state,
        &[muse(
            100.0,
            Some("2026-10-18T12:00:00Z"),
            "2026-10-18T10:00:00Z",
        )],
    );
    assert_eq!(
        exhausted_again
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        [LimitEventKind::Exhausted]
    );
}

#[test]
fn a_drop_read_before_the_saved_reset_is_not_a_reset() {
    let mut state = MonitorState::default();
    let at_limit = snapshot(
        AgentId::Muse,
        "profile:you",
        "you@example.com",
        100.0,
        Some("2026-08-19T15:00:00Z"),
    );
    assert!(observe(&mut state, &[at_limit]).is_empty());

    let early = observe(
        &mut state,
        &[observed_at(
            snapshot(AgentId::Muse, "profile:you", "you@example.com", 0.0, None),
            "2026-08-19T14:00:00Z",
        )],
    );

    assert!(early.is_empty());
    assert!(state.providers[&AgentId::Muse].windows["weekly"].exhausted);
}

#[test]
fn advancing_the_reset_timestamp_after_usage_notifies_once() {
    let mut state = MonitorState::default();
    let before = snapshot(
        AgentId::Claude,
        "account-a",
        "me@example.com",
        82.0,
        Some("2026-08-24T12:00:00Z"),
    );
    let after = observed_at(
        snapshot(
            AgentId::Claude,
            "account-a",
            "me@example.com",
            0.0,
            Some("2026-08-31T12:00:00Z"),
        ),
        "2026-08-19T13:00:00Z",
    );
    assert!(observe(&mut state, &[before]).is_empty());

    let events = observe(&mut state, std::slice::from_ref(&after));

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, LimitEventKind::Reset);
    assert_eq!(events[0].provider, AgentId::Claude);
    assert_eq!(events[0].account_label.as_deref(), Some("me@example.com"));
    assert_eq!(events[0].window_label, "Weekly · all models");
    assert_eq!(events[0].previous_used_percent, 82.0);
    assert_eq!(events[0].used_percent, 0.0);
    assert!(observe(&mut state, &[after]).is_empty());
}

#[test]
fn a_reset_time_read_a_second_apart_is_not_a_reset() {
    for jittered in [
        "2026-08-24T12:00:01Z",
        "2026-08-24T11:59:59.000Z",
        "2026-08-24T12:00:00.000+00:00",
    ] {
        let mut state = MonitorState::default();
        let before = snapshot(
            AgentId::Codex,
            "account-a",
            "me@example.com",
            80.0,
            Some("2026-08-24T12:00:00Z"),
        );
        assert!(observe(&mut state, &[before]).is_empty());

        let events = observe(
            &mut state,
            &[observed_at(
                snapshot(
                    AgentId::Codex,
                    "account-a",
                    "me@example.com",
                    79.0,
                    Some(jittered),
                ),
                "2026-08-19T13:00:00Z",
            )],
        );

        assert!(events.is_empty(), "{jittered}: {events:?}");
    }
}

#[test]
fn an_older_observation_never_notifies_or_replaces_the_baseline() {
    let mut state = MonitorState::default();
    let baseline = observed_at(
        snapshot(
            AgentId::Codex,
            "account-a",
            "me@example.com",
            80.0,
            Some("2026-08-24T12:00:00Z"),
        ),
        "2026-08-19T13:00:00Z",
    );
    let stale = observed_at(
        snapshot(
            AgentId::Codex,
            "account-a",
            "me@example.com",
            100.0,
            Some("2026-08-24T12:00:00Z"),
        ),
        "2026-08-19T12:00:00Z",
    );
    assert!(observe(&mut state, &[baseline]).is_empty());

    assert!(observe(&mut state, &[stale]).is_empty());
    assert_eq!(
        state.providers[&AgentId::Codex].windows["weekly"].used_percent,
        80.0
    );
}

#[test]
fn switching_accounts_establishes_a_new_baseline() {
    let mut state = MonitorState::default();
    assert!(observe(
        &mut state,
        &[snapshot(
            AgentId::Claude,
            "account-a",
            "first@example.com",
            82.0,
            Some("2026-08-24T12:00:00Z"),
        )],
    )
    .is_empty());

    assert!(observe(
        &mut state,
        &[snapshot(
            AgentId::Claude,
            "account-b",
            "second@example.com",
            70.0,
            Some("2026-08-25T12:00:00Z"),
        )],
    )
    .is_empty());

    let events = observe(
        &mut state,
        &[observed_at(
            snapshot(
                AgentId::Claude,
                "account-b",
                "second@example.com",
                0.0,
                Some("2026-09-01T12:00:00Z"),
            ),
            "2026-08-19T13:00:00Z",
        )],
    );
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].account_label.as_deref(),
        Some("second@example.com")
    );
}

#[test]
fn failed_and_remembered_snapshots_do_not_replace_the_notification_baseline() {
    let mut state = MonitorState::default();
    let before = snapshot(
        AgentId::Claude,
        "account-a",
        "me@example.com",
        82.0,
        Some("2026-08-24T12:00:00Z"),
    );
    assert!(observe(&mut state, std::slice::from_ref(&before)).is_empty());

    let mut failed = before.clone();
    failed.status = LimitsStatus::Failed;
    failed.reading.windows.clear();
    let mut remembered = before;
    remembered.current_account = false;
    remembered.reading.windows[0].used_percent = 0.0;
    assert!(observe(&mut state, &[failed, remembered]).is_empty());

    let events = observe(
        &mut state,
        &[observed_at(
            snapshot(
                AgentId::Claude,
                "account-a",
                "me@example.com",
                0.0,
                Some("2026-08-31T12:00:00Z"),
            ),
            "2026-08-19T13:00:00Z",
        )],
    );
    assert_eq!(events.len(), 1);
}

#[test]
fn persisted_observations_prevent_duplicate_notifications_after_restart() {
    let root = scratch_dir("limits-monitor-round-trip");
    let path = root.join("monitor.json");
    let mut state = MonitorState::default();
    let before = snapshot(
        AgentId::Claude,
        "account-a",
        "me@example.com",
        82.0,
        Some("2026-08-24T12:00:00Z"),
    );
    let after = observed_at(
        snapshot(
            AgentId::Claude,
            "account-a",
            "me@example.com",
            0.0,
            Some("2026-08-31T12:00:00Z"),
        ),
        "2026-08-19T13:00:00Z",
    );
    assert!(observe(&mut state, &[before]).is_empty());
    assert_eq!(observe(&mut state, std::slice::from_ref(&after)).len(), 1);
    monitor::save_state(&path, &state).unwrap();

    let mut reloaded = load_state(&path);

    assert!(observe(&mut reloaded, &[after]).is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn malformed_monitor_state_falls_back_to_an_empty_baseline() {
    let root = scratch_dir("limits-monitor-malformed");
    let path = root.join("monitor.json");
    fs::write(&path, "{nope").unwrap();

    let state = load_state(&path);

    assert!(state.providers.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn monitor_state_without_observation_times_is_discarded_instead_of_migrated() {
    let root = scratch_dir("limits-monitor-legacy");
    let path = root.join("monitor.json");
    fs::write(
        &path,
        r#"{"providers":{"claude":{"account_id":"account-a","windows":{"weekly":{"used_percent":99,"resets_at":"2026-08-24T12:00:00Z","exhausted":false}}}}}"#,
    )
    .unwrap();

    let state = load_state(&path);

    assert!(state.providers.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_monitor_reads_only_what_its_settings_watch() {
    let alert = crate::reset_alert::ResetAlert::default();
    let settings = |limit_notifications: bool, alerts: bool| crate::settings::AppSettings {
        limit_notifications,
        reset_alerts: if alerts {
            HashMap::from([("acct".to_string(), alert.clone())])
        } else {
            HashMap::new()
        },
        ..crate::settings::AppSettings::default()
    };

    assert_eq!(
        watched_providers(&settings(false, false)),
        &[] as &[AgentId]
    );
    assert_eq!(
        watched_providers(&settings(false, true)),
        crate::provider_catalogue::WITH_BANKED_RESETS
    );
    assert_eq!(
        watched_providers(&settings(true, false)),
        crate::provider_catalogue::WITH_LIMITS
    );
    assert_eq!(
        watched_providers(&settings(true, true)),
        crate::provider_catalogue::WITH_LIMITS
    );
}

fn codex_with_a_reset(used: f64, at: &str) -> ProviderLimitsDto {
    let mut card = observed_at(
        snapshot(
            AgentId::Codex,
            "acct-codex",
            "you@example.com",
            used,
            Some("2026-08-24T12:00:00Z"),
        ),
        at,
    );
    card.reading.reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 1,
        next_expires_at: None,
        resets: Vec::new(),
    });
    card
}

fn alerts_only() -> crate::settings::AppSettings {
    crate::settings::AppSettings {
        limit_notifications: false,
        reset_alerts: HashMap::from([(
            "acct-codex".to_string(),
            crate::reset_alert::ResetAlert::default(),
        )]),
        ..crate::settings::AppSettings::default()
    }
}

#[test]
fn a_state_written_before_alerts_keeps_its_limit_baselines() {
    let root = scratch_dir("limits-monitor-before-alerts");
    let path = root.join("monitor.json");
    fs::write(
        &path,
        r#"{"schema_version":2,"providers":{"claude":{"account_id":"account-a","windows":{"weekly":{"used_percent":50,"resets_at":"2026-08-24T12:00:00Z","observed_at":"2026-08-19T12:00:00Z","exhausted":false}}}}}"#,
    )
    .unwrap();

    let state = load_state(&path);

    assert_eq!(state.providers[&AgentId::Claude].account_id, "account-a");
    assert!(state.reset_alerts.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_state_file_with_alerts_reads_and_writes_back_as_it_was() {
    let root = scratch_dir("limits-monitor-alert-shape");
    let path = root.join("monitor.json");
    let document = serde_json::json!({
        "schema_version": 2,
        "providers": {"codex": {"account_id": "acct-codex", "windows": {"weekly": {
            "used_percent": 95.0,
            "resets_at": "2026-08-24T12:00:00+00:00",
            "observed_at": "2026-08-19T12:00:00Z",
            "exhausted": false
        }}}},
        "reset_alerts": {
            "acct-codex": {
                "low": {"cycle": "2026-08-24T12:00:00+00:00", "observedAt": "2026-08-19T12:00:00Z"},
                "offeredCycle": "2026-08-17T12:00:00+00:00"
            },
            "acct-other": {"low": null, "offeredCycle": null}
        }
    });
    fs::write(&path, document.to_string()).unwrap();

    let state = load_state(&path);
    monitor::save_state(&path, &state).unwrap();

    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written, document);
    let _ = fs::remove_dir_all(root);
}

fn automatic_only() -> crate::settings::AppSettings {
    let mut settings = alerts_only();
    settings
        .reset_alerts
        .get_mut("acct-codex")
        .unwrap()
        .automatic = true;
    settings
}

fn at(value: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&chrono::Utc)
}

#[test]
fn the_monitor_wakes_when_a_reset_falls_due_but_never_every_second() {
    let now = at("2026-08-19T13:00:00Z");
    let poll = Duration::from_secs(300);

    assert_eq!(poll, minutes(5));
    assert_eq!(next_wake(poll, None, now, now), poll);
    assert_eq!(
        next_wake(poll, Some(at("2026-08-19T13:10:00Z")), now, now),
        poll,
        "a spend due after the next poll skips that poll"
    );
    assert_eq!(
        next_wake(poll, Some(at("2026-08-19T13:02:00Z")), now, now),
        Duration::from_secs(120)
    );
    assert_eq!(
        next_wake(
            poll,
            Some(at("2026-08-19T13:00:30Z")),
            now,
            at("2026-08-19T13:01:00Z")
        ),
        Duration::from_secs(1),
        "one that fell due while the poll ran wakes it at once, but no faster than a second"
    );
    assert_eq!(
        next_wake(poll, Some(at("2026-08-19T12:55:00Z")), now, now),
        poll,
        "one the poll saw and could not decide waits for the next poll"
    );
}

#[test]
fn limits_checks_wake_the_monitor_in_five_minutes_without_delaying_a_reset() {
    let now = at("2026-08-19T13:00:00Z");
    let poll = crate::limits_refresh::check_interval(minutes(30));
    assert_eq!(next_wake(poll, None, now, now), Duration::from_secs(300));
    assert_eq!(
        next_wake(poll, Some(at("2026-08-19T13:02:00Z")), now, now),
        Duration::from_secs(120)
    );
    let faster = crate::limits_refresh::check_interval(minutes(2));
    assert_eq!(next_wake(faster, None, now, now), Duration::from_secs(120));
}

mod poll;
