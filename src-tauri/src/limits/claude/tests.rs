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

/// The saved-reset block as Claude Code 2.1.280's own reader expects it under `cedar_ember` on
/// `GET /api/oauth/usage?cedar_ember=1&skip_spend=1`. Reconstructed from that reader's schema, not
/// captured: no live answer has been observed on this machine yet.
fn with_saved_resets(block: serde_json::Value) -> serde_json::Value {
    json!({
        "limits": [{"kind": "session", "group": "session", "percent": 100}],
        "cedar_ember": block
    })
}

fn grant(id: &str, resets_left: u32, ends_at: Option<&str>) -> serde_json::Value {
    json!({
        "id": id, "label": "Saved reset", "resets_total": 1, "resets_left": resets_left,
        "starts_at": "2026-09-01T00:00:00Z", "ends_at": ends_at,
        "clears": ["five_hour", "seven_day"], "paused": false, "usable_now": true,
        "use_requires_limit": false, "percent_used": {"five_hour": 100}, "blocking": ["five_hour"]
    })
}

#[test]
fn saved_resets_count_every_grant_and_expire_with_the_soonest_one_still_holding_a_reset() {
    let payload = with_saved_resets(json!({
        "eligible": true,
        "at_limit": true,
        "grants": [
            grant("later", 1, Some("2026-10-30T00:00:00Z")),
            grant("spent", 0, Some("2026-09-25T00:00:00Z")),
            grant("sooner", 2, Some("2026-10-05T12:00:00+02:00")),
            grant("undated", 1, None)
        ],
        "next_grant_id": "sooner"
    }));

    assert_eq!(
        parse_reset_credits(&payload),
        Some(LimitsResetCreditsDto {
            available_count: 4,
            next_expires_at: Some("2026-10-05T10:00:00+00:00".to_string()),
        })
    );
}

#[test]
fn a_read_without_a_usable_saved_reset_block_is_unknown_rather_than_zero() {
    let windows_only = json!({"limits": [{"kind": "session", "group": "session", "percent": 5}]});
    assert_eq!(parse_reset_credits(&windows_only), None);
    assert_eq!(parse_reset_credits(&with_saved_resets(json!(null))), None);
    assert_eq!(parse_reset_credits(&with_saved_resets(json!("on"))), None);
    let unanswered = with_saved_resets(json!({
        "eligible": false, "ineligible_reason": "unavailable", "grants": []
    }));
    assert_eq!(
        parse_reset_credits(&unanswered),
        None,
        "Claude Code treats `unavailable` as a failed read, not as holding nothing"
    );
}

#[test]
fn an_answered_block_without_resets_reports_zero_so_a_remembered_count_is_replaced() {
    for block in [
        json!({"eligible": false, "ineligible_reason": "no_grant"}),
        json!({"eligible": true, "grants": []}),
        json!({"eligible": true, "grants": [grant("used", 0, Some("2026-10-05T00:00:00Z"))]}),
    ] {
        assert_eq!(
            parse_reset_credits(&with_saved_resets(block.clone())),
            Some(LimitsResetCreditsDto {
                available_count: 0,
                next_expires_at: None,
            }),
            "{block}"
        );
    }
}

#[test]
fn malformed_grants_are_skipped_without_losing_the_well_formed_ones() {
    let payload = with_saved_resets(json!({
        "eligible": true,
        "grants": [
            "not an object",
            {"id": "no-count", "ends_at": "2026-09-30T00:00:00Z"},
            {"id": "negative", "resets_left": -1, "ends_at": "2026-09-30T00:00:00Z"},
            {"id": "fraction", "resets_left": 1.5, "ends_at": "2026-09-30T00:00:00Z"},
            grant("bad-date", 1, Some("next tuesday")),
            grant("good", 1, Some("2026-10-01T00:00:00Z"))
        ]
    }));

    assert_eq!(
        parse_reset_credits(&payload),
        Some(LimitsResetCreditsDto {
            available_count: 2,
            next_expires_at: Some("2026-10-01T00:00:00+00:00".to_string()),
        })
    );
}
