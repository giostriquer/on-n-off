//! The windows of a Claude usage report (`parse_claude`): the normalized `limits[]` array, and the
//! legacy top-level fields it replaced.
use super::*;
use serde_json::json;

/// Sanitised capture from 2026-08-17 (Max plan). Legacy fields *and* the normalized
/// `limits[]` array are present; the legacy numbers are deliberately different so the
/// test proves which one wins.
const CAPTURED: &str = r#"{
      "five_hour": {"utilization": 99.0, "resets_at": "2026-08-18T04:59:59.692639+00:00"},
      "seven_day": {"utilization": 98.0, "resets_at": "2026-08-24T13:59:59.692659+00:00"},
      "seven_day_oauth_apps": null,
      "seven_day_opus": null,
      "seven_day_sonnet": null,
      "nimbus_quill": {"utilization": 0.0, "resets_at": null},
      "extra_usage": {"is_enabled": false, "monthly_limit": null, "used_credits": 0.0, "utilization": null},
      "limits": [
        {"kind": "session", "group": "session", "percent": 7, "severity": "normal",
         "resets_at": "2026-08-18T04:59:59.692639+00:00", "scope": null, "is_active": false},
        {"kind": "weekly_all", "group": "weekly", "percent": 12, "severity": "normal",
         "resets_at": "2026-08-24T13:59:59.692659+00:00", "scope": null, "is_active": false}
      ],
      "spend": {"used": {"amount_minor": 0, "currency": "BRL", "exponent": 2}, "limit": null},
      "member_dashboard_available": false
    }"#;

#[test]
fn prefers_the_normalized_limits_array_over_legacy_fields() {
    let payload: serde_json::Value = serde_json::from_str(CAPTURED).unwrap();
    let windows = parse_claude(&payload);
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].id, "session");
    assert_eq!(windows[0].kind, LimitWindowKind::Session);
    assert_eq!(windows[0].label, "5 hour · all models");
    assert_eq!(windows[0].used_percent, 7.0);
    assert_eq!(
        windows[0].resets_at.as_deref(),
        Some("2026-08-18T04:59:59.692639+00:00")
    );
    assert_eq!(windows[1].id, "weekly_all");
    assert_eq!(windows[1].kind, LimitWindowKind::Weekly);
    assert_eq!(windows[1].label, "Weekly · all models");
    assert_eq!(windows[1].used_percent, 12.0);
}

#[test]
fn weekly_group_kinds_other_than_all_are_model_windows() {
    let payload = json!({
        "limits": [
            {"kind": "weekly_opus", "group": "weekly", "percent": 40, "resets_at": null},
            {"kind": "weekly_sonnet_4_5", "group": "weekly", "percent": 3.5}
        ]
    });
    let windows = parse_claude(&payload);
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].kind, LimitWindowKind::Model);
    assert_eq!(windows[0].label, "Weekly · Opus");
    assert_eq!(windows[0].resets_at, None);
    assert_eq!(windows[1].label, "Weekly · Sonnet 4 5");
    assert_eq!(windows[1].used_percent, 3.5);
}

#[test]
fn scoped_weekly_limits_are_labelled_by_their_scope_and_keep_distinct_ids() {
    let payload = json!({
        "limits": [
            {"kind": "weekly_scoped", "group": "weekly", "percent": 25,
             "scope": {"model": {"id": null, "display_name": "Fable"}, "surface": null}},
            {"kind": "weekly_scoped", "group": "weekly", "percent": 4,
             "scope": {"model": null, "surface": "cowork"}},
            {"kind": "weekly_scoped", "group": "weekly", "percent": 1, "scope": null}
        ]
    });
    let windows = parse_claude(&payload);
    let summary: Vec<(&str, &str)> = windows
        .iter()
        .map(|w| (w.id.as_str(), w.label.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            ("weekly_scoped:Fable", "Weekly · Fable"),
            ("weekly_scoped:cowork", "Weekly · cowork"),
            ("weekly_scoped", "Weekly · Scoped"),
        ]
    );
    assert!(windows.iter().all(|w| w.kind == LimitWindowKind::Model));
}

#[test]
fn falls_back_to_legacy_fields_when_limits_array_is_absent() {
    let payload = json!({
        "five_hour": {"utilization": 7.0, "resets_at": "2026-08-18T04:59:59+00:00"},
        "seven_day": {"utilization": 12.0, "resets_at": "2026-08-24T13:59:59+00:00"},
        "seven_day_opus": {"utilization": 30.0, "resets_at": "2026-08-24T13:59:59+00:00"},
        "seven_day_sonnet": null
    });
    let windows = parse_claude(&payload);
    let ids: Vec<&str> = windows.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["session", "weekly_all", "weekly_opus"]);
    assert_eq!(windows[2].kind, LimitWindowKind::Model);
    assert_eq!(windows[2].label, "Weekly · Opus");
    assert_eq!(windows[2].used_percent, 30.0);
    assert_eq!(
        windows[0].resets_at.as_deref(),
        Some("2026-08-18T04:59:59+00:00")
    );
}

#[test]
fn clamps_percentages_and_skips_malformed_entries() {
    let payload = json!({
        "limits": [
            {"kind": "session", "group": "session", "percent": 140},
            {"kind": "weekly_all", "group": "weekly", "percent": -5},
            {"kind": "weekly_opus", "group": "weekly", "percent": "lots"},
            {"kind": "weekly_haiku", "group": "weekly"},
            {"kind": "mystery", "group": "monthly", "percent": 10},
            "not an object",
            {"group": "weekly", "percent": 10}
        ]
    });
    let windows = parse_claude(&payload);
    let summary: Vec<(&str, f64)> = windows
        .iter()
        .map(|w| (w.id.as_str(), w.used_percent))
        .collect();
    assert_eq!(summary, [("session", 100.0), ("weekly_all", 0.0)]);
}

#[test]
fn empty_limits_array_falls_back_to_legacy_fields() {
    let payload = json!({
        "limits": [],
        "five_hour": {"utilization": 1.0, "resets_at": null}
    });
    let windows = parse_claude(&payload);
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].id, "session");
}

/// The signed-in card is Claude Code's own report for the user's login, as the environment places
/// it (here a test build's disposable home), read in a config dir of on-n-off's own, which holds
/// none of the transcripts the report would otherwise scan.
#[test]
fn the_signed_in_card_is_claude_codes_report_read_in_a_config_dir_without_history() {
    use crate::cli_stub::CliStub;
    let home = crate::paths::scratch_dir("limits-claude-signed-in");
    std::fs::write(
        home.join(".claude.json"),
        json!({"oauthAccount": {"accountUuid": "user", "organizationUuid": "team",
                                "emailAddress": "you@example.com"}})
        .to_string(),
    )
    .unwrap();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("report.jsonl"),
        r#"{"type":"assistant","usage_report":{"rate_limits":{"limits":[{"kind":"weekly_all","group":"weekly","percent":34,"resets_at":"2026-10-05T09:00:00+00:00"}]}}}"#,
    )
    .unwrap();
    let stub = CliStub::new("claude")
        .log_args("args.txt", false)
        .log_env("CLAUDE_CONFIG_DIR", "config-dir.txt")
        .stdout_file("report.jsonl")
        .write(&bin);

    let cards = crate::accounts::native::with_test_cli(&stub, || {
        crate::limits::read_limits_at(AgentId::Claude, false, &home)
    });

    let card = &cards[0];
    assert_eq!(card.status, LimitsStatus::Ok, "{:?}", card.message);
    assert!(card.current_account);
    let identity = crate::accounts::model::Identity {
        provider: AgentId::Claude,
        user_id: "user".into(),
        workspace_id: "team".into(),
    };
    assert_eq!(
        card.account.as_ref().map(|account| account.id.as_str()),
        Some(identity.observation_key().as_str())
    );
    assert_eq!(card.reading.windows.len(), 1);
    let args = std::fs::read_to_string(bin.join("args.txt")).unwrap();
    assert!(args.contains("--safe-mode"), "{args}");
    let config_dir = std::fs::read_to_string(bin.join("config-dir.txt")).unwrap();
    let own = home.join(".on-n-off").join("claude-usage");
    assert_eq!(std::path::Path::new(config_dir.trim()), own.as_path());
    assert!(own.is_dir());
    let _ = std::fs::remove_dir_all(&home);
}

/// A user who has not run Claude Code yet has no config dir for it to work in: the read still runs,
/// and reads the store as signed out.
#[test]
fn a_config_dir_claude_code_has_not_made_yet_still_asks_it() {
    use crate::cli_stub::CliStub;
    let home = crate::paths::scratch_dir("limits-claude-no-config-dir");
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    // Signed out, Claude Code prints no report from `/usage`, and says so from `auth status`.
    let stub = CliStub::new("claude")
        .stdout(r#"{"loggedIn":false,"authMethod":"none"}"#)
        .write(&bin);
    assert!(!home.join(".claude").exists());

    let cards = crate::accounts::native::with_test_cli(&stub, || {
        crate::limits::read_limits_at(AgentId::Claude, false, &home)
    });

    assert_eq!(
        cards[0].status,
        LimitsStatus::SignedOut,
        "{:?}",
        cards[0].message
    );
    // Claude Code answered: the card is not the one a missing `claude` gets.
    assert_eq!(
        cards[0].message.as_deref(),
        Some("Sign in with `claude` to see subscription limits.")
    );
    let _ = std::fs::remove_dir_all(&home);
}

/// Without Claude Code there is nothing to read, and the card says what would be.
#[test]
fn without_claude_code_the_card_asks_for_it() {
    let home = crate::paths::scratch_dir("limits-claude-not-installed");
    let missing = home.join("no-such-claude");

    let cards = crate::accounts::native::with_test_cli(&missing, || {
        crate::limits::read_limits_at(AgentId::Claude, false, &home)
    });

    assert_eq!(cards[0].status, LimitsStatus::SignedOut);
    assert_eq!(
        cards[0].message.as_deref(),
        Some("Install Claude Code and sign in with `claude` to see subscription limits.")
    );
    let _ = std::fs::remove_dir_all(&home);
}
