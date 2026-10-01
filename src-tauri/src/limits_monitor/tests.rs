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

/// Codex's hidden buckets never reach a surface, so one reaching its limit is no reason to notify.
/// The Codex reader drops them before the monitor sees the read; the weekly window beside it still
/// notifies.
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
fn a_large_drop_without_a_new_reset_instant_is_not_a_reset() {
    let mut state = MonitorState::default();
    let reset_at = Some("2026-08-24T12:00:00Z");
    assert!(observe(
        &mut state,
        &[snapshot(
            AgentId::Codex,
            "account-a",
            "me@example.com",
            80.0,
            reset_at,
        )],
    )
    .is_empty());

    let events = observe(
        &mut state,
        &[observed_at(
            snapshot(AgentId::Codex, "account-a", "me@example.com", 8.0, reset_at),
            "2026-08-19T13:00:00Z",
        )],
    );

    assert!(events.is_empty());
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
fn a_reset_instant_that_moves_without_usage_dropping_is_not_a_reset() {
    let mut state = MonitorState::default();
    assert!(observe(
        &mut state,
        &[snapshot(
            AgentId::Codex,
            "account-a",
            "me@example.com",
            50.0,
            Some("2026-08-24T12:00:00Z"),
        )],
    )
    .is_empty());

    let events = observe(
        &mut state,
        &[observed_at(
            snapshot(
                AgentId::Codex,
                "account-a",
                "me@example.com",
                49.5,
                Some("2026-08-24T12:05:00Z"),
            ),
            "2026-08-19T13:00:00Z",
        )],
    );

    assert!(events.is_empty());
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
fn failure_backoff_doubles_and_caps_at_sixty_minutes() {
    assert_eq!(poll_delay_minutes(10, 0), 10);
    assert_eq!(poll_delay_minutes(10, 1), 20);
    assert_eq!(poll_delay_minutes(10, 2), 40);
    assert_eq!(poll_delay_minutes(10, 3), 60);
    assert_eq!(poll_delay_minutes(10, 8), 60);
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

/// A banked reset alert is Codex's alone, so with limit notifications off an alert reads Codex and
/// never Claude, whose failures would otherwise slow the alert's polls.
#[test]
fn the_monitor_reads_only_what_its_settings_watch() {
    let alert = crate::settings::ResetAlert {
        label: None,
        max_left_percent: 10,
        min_hours_to_renewal: 24,
        automatic: false,
    };
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
    assert_eq!(watched_providers(&settings(false, true)), &[AgentId::Codex]);
    assert_eq!(
        watched_providers(&settings(true, false)),
        &[AgentId::Claude, AgentId::Codex]
    );
    assert_eq!(
        watched_providers(&settings(true, true)),
        &[AgentId::Claude, AgentId::Codex]
    );
}

/// A signed-in Codex card at `used` observed `at`, with a banked reset to spend.
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
            crate::settings::ResetAlert {
                label: None,
                max_left_percent: 10,
                min_hours_to_renewal: 24,
                automatic: false,
            },
        )]),
        ..crate::settings::AppSettings::default()
    }
}

/// With limit notifications off, an alert still offers its reset, and a limit reaching 100% on the
/// same reads notifies nothing.
#[test]
fn an_alert_offers_its_reset_while_limit_notifications_stay_quiet() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-19T13:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let mut state = MonitorState::default();
    let settings = alerts_only();

    let first = notices_of(
        &mut state,
        &[codex_with_a_reset(95.0, "2026-08-19T12:00:00Z")],
        &settings,
        now,
    );
    let second = notices_of(
        &mut state,
        &[codex_with_a_reset(100.0, "2026-08-19T12:10:00Z")],
        &settings,
        now,
    );

    assert!(first.is_empty());
    assert_eq!(
        second
            .iter()
            .map(|(title, _)| title.as_str())
            .collect::<Vec<_>>(),
        ["Codex: a banked reset is available"]
    );
    assert!(state.providers.is_empty(), "limit baselines kept while off");
}

/// With limit notifications on, the same reads notify the limit reached as well.
#[test]
fn with_limit_notifications_on_the_limit_is_notified_too() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-19T13:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let mut state = MonitorState::default();
    let settings = crate::settings::AppSettings {
        limit_notifications: true,
        ..alerts_only()
    };

    notices_of(
        &mut state,
        &[codex_with_a_reset(95.0, "2026-08-19T12:00:00Z")],
        &settings,
        now,
    );
    let notices = notices_of(
        &mut state,
        &[codex_with_a_reset(100.0, "2026-08-19T12:10:00Z")],
        &settings,
        now,
    );

    let titles: Vec<&str> = notices.iter().map(|(title, _)| title.as_str()).collect();
    assert_eq!(
        titles,
        ["Codex limit reached", "Codex: a banked reset is available"]
    );
}

/// With nothing watched the monitor reads nothing, and that poll of nothing forgets everything it
/// observed, so turning a notification back on starts from a fresh baseline.
#[test]
fn a_poll_of_nothing_forgets_every_observation() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-19T13:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let mut state = MonitorState::default();
    let on = crate::settings::AppSettings {
        limit_notifications: true,
        ..alerts_only()
    };
    notices_of(
        &mut state,
        &[codex_with_a_reset(95.0, "2026-08-19T12:00:00Z")],
        &on,
        now,
    );
    assert!(!state.providers.is_empty() && !state.reset_alerts.is_empty());

    let notices = notices_of(
        &mut state,
        &[],
        &crate::settings::AppSettings::default(),
        now,
    );

    assert!(notices.is_empty());
    assert!(state.providers.is_empty() && state.reset_alerts.is_empty());
}

/// A state file written before banked reset alerts existed still loads, limit baselines and all.
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

/// An offer made before a restart is not made again after it.
#[test]
fn an_offer_is_not_made_again_after_a_restart() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-19T13:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let root = scratch_dir("limits-monitor-offer-restart");
    let path = root.join("monitor.json");
    let settings = alerts_only();
    let mut state = MonitorState::default();
    notices_of(
        &mut state,
        &[codex_with_a_reset(95.0, "2026-08-19T12:00:00Z")],
        &settings,
        now,
    );
    assert_eq!(
        notices_of(
            &mut state,
            &[codex_with_a_reset(96.0, "2026-08-19T12:10:00Z")],
            &settings,
            now
        )
        .len(),
        1
    );
    monitor::save_state(&path, &state).unwrap();

    let mut reloaded = load_state(&path);

    assert!(notices_of(
        &mut reloaded,
        &[codex_with_a_reset(97.0, "2026-08-19T12:20:00Z")],
        &settings,
        now
    )
    .is_empty());
    let _ = fs::remove_dir_all(root);
}

/// What one poll notifies, for an alert that only notifies: nothing is offered to be spent.
fn notices_of(
    state: &mut MonitorState,
    snapshots: &[ProviderLimitsDto],
    settings: &crate::settings::AppSettings,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<(String, String)> {
    let outcome = observe_poll(state, snapshots, settings, now);
    assert!(outcome.automatic_offers.is_empty());
    outcome.notices
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

/// An automatic alert's offer is not the "available" notification: it is handed on to be
/// scheduled, once it is saved.
#[test]
fn an_automatic_alerts_offer_is_handed_on_to_be_scheduled() {
    let now = at("2026-08-19T13:00:00Z");
    let mut state = MonitorState::default();
    let settings = automatic_only();

    observe_poll(
        &mut state,
        &[codex_with_a_reset(95.0, "2026-08-19T12:00:00Z")],
        &settings,
        now,
    );
    let outcome = observe_poll(
        &mut state,
        &[codex_with_a_reset(96.0, "2026-08-19T12:10:00Z")],
        &settings,
        now,
    );

    assert!(outcome.notices.is_empty(), "offered as well as scheduled");
    assert_eq!(outcome.automatic_offers.len(), 1);
    assert_eq!(outcome.automatic_offers[0].account_id, "acct-codex");
}

/// The monitor wakes when a reset falls due, not at its next poll, and polls as usual otherwise. A
/// spend its last poll saw and could not decide waits for the next poll, which after a failure is
/// the backoff, so the monitor never asks every second.
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
