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
/// captured: the one live answer seen so far said only that the account holds none.
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

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-22T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn resets(available_count: u32, next_expires_at: Option<&str>) -> Option<LimitsResetCreditsDto> {
    Some(LimitsResetCreditsDto {
        available_count,
        next_expires_at: next_expires_at.map(str::to_string),
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
        parse_reset_credits(&payload, now()),
        resets(4, Some("2026-10-05T10:00:00+00:00"))
    );
}

#[test]
fn a_grant_that_already_ended_is_counted_but_never_named_as_the_next_expiry() {
    let payload = with_saved_resets(json!({
        "eligible": true,
        "grants": [
            grant("ended", 1, Some("2026-09-20T00:00:00Z")),
            grant("live", 1, Some("2026-10-05T00:00:00Z"))
        ]
    }));

    assert_eq!(
        parse_reset_credits(&payload, now()),
        resets(2, Some("2026-10-05T00:00:00+00:00")),
        "Claude Code sums every grant's resets_left but offers only a grant that has not ended"
    );
}

#[test]
fn usage_parses_the_windows_and_the_saved_resets_from_one_payload() {
    let payload = with_saved_resets(json!({
        "eligible": true,
        "grants": [grant("launch", 1, Some("2026-10-05T00:00:00Z"))]
    }));

    let parsed = parse_usage(&payload, now());

    assert_eq!(parsed.windows.len(), 1);
    assert_eq!(parsed.windows[0].id, "session");
    assert_eq!(
        parsed.reset_credits,
        resets(1, Some("2026-10-05T00:00:00+00:00"))
    );
}

#[test]
fn a_read_without_a_usable_saved_reset_block_is_unknown_rather_than_zero() {
    let windows_only = json!({"limits": [{"kind": "session", "group": "session", "percent": 5}]});
    assert_eq!(parse_reset_credits(&windows_only, now()), None);
    for block in [
        json!(null),
        json!("on"),
        // Claude Code treats `unavailable` as a failed read, not as holding nothing.
        json!({"eligible": false, "ineligible_reason": "unavailable", "grants": []}),
        // These say who is asking, not what the account holds: on-n-off is not Claude Code.
        json!({"eligible": false, "ineligible_reason": "surface"}),
        json!({"eligible": false, "ineligible_reason": "cli_version", "grants": []}),
        json!({"eligible": false, "ineligible_reason": "mobile"}),
        json!({"eligible": false, "ineligible_reason": "unknown"}),
        json!({"eligible": false, "ineligible_reason": "a_reason_added_later"}),
        json!({"eligible": false}),
        // Grants that were sent but cannot be read are not an answer either.
        json!({"eligible": true, "grants": "none"}),
        json!({"eligible": true, "grants": [{"id": "no-count"}, {"id": "text", "resets_left": "1"}]}),
        json!({"eligible": true, "grants": [{"id": "negative", "resets_left": -1}]}),
        // An unanswered status stays unknown even when it lists grants.
        json!({"eligible": false, "ineligible_reason": "unavailable", "grants": [grant("g", 1, None)]}),
    ] {
        assert_eq!(
            parse_reset_credits(&with_saved_resets(block.clone()), now()),
            None,
            "{block}"
        );
    }
}

#[test]
fn an_answered_block_without_resets_reports_zero_so_a_remembered_count_is_replaced() {
    let mut blocks = vec![
        json!({"eligible": true}),
        json!({"eligible": true, "grants": []}),
        json!({"eligible": true, "grants": [grant("used", 0, Some("2026-10-05T00:00:00Z"))]}),
    ];
    // These describe the account or the program, so the account really holds none.
    for reason in [
        "no_grant",
        "tier",
        "seat",
        "tenure",
        "other_experiment",
        "config_off",
    ] {
        blocks.push(json!({"eligible": false, "ineligible_reason": reason}));
    }
    for block in blocks {
        assert_eq!(
            parse_reset_credits(&with_saved_resets(block.clone()), now()),
            resets(0, None),
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
            {"id": "text", "resets_left": "1", "ends_at": "2026-09-30T00:00:00Z"},
            {"id": "whole-float", "resets_left": 1.0, "ends_at": "2026-10-02T00:00:00Z"},
            grant("bad-date", 1, Some("next tuesday")),
            grant("good", 1, Some("2026-10-01T00:00:00Z"))
        ]
    }));

    assert_eq!(
        parse_reset_credits(&payload, now()),
        resets(3, Some("2026-10-01T00:00:00+00:00")),
        "a whole number sent as 1.0 is still one reset, as Claude Code's reader accepts it"
    );
}

#[test]
fn a_count_too_large_for_the_card_saturates_instead_of_wrapping_or_dropping_a_grant() {
    let payload = with_saved_resets(json!({
        "eligible": true,
        "grants": [
            {"id": "huge", "resets_left": 1_099_511_627_776_u64},
            {"id": "one", "resets_left": 1}
        ]
    }));

    // The oversized grant comes first, so dropping it or zeroing it leaves 1, and wrapping the sum
    // leaves 0; only a clamp and a saturating sum reach u32::MAX.
    assert_eq!(parse_reset_credits(&payload, now()), resets(u32::MAX, None));
}

/// The profile's `organization.subscription_status` is kept as Anthropic writes it; a missing,
/// empty or non-text value is simply unknown, and never fails the profile.
#[test]
fn reads_the_subscription_status_the_profile_reports() {
    let status = |value: serde_json::Value| {
        parse_profile(
            &json!({"account":{"uuid":"u"},"organization":{"uuid":"o","subscription_status":value}}),
        )
        .expect("the profile still reads")
        .subscription_status
    };

    assert_eq!(status(json!("past_due")).as_deref(), Some("past_due"));
    assert_eq!(status(json!(" active ")).as_deref(), Some("active"));
    assert_eq!(status(json!("")), None);
    assert_eq!(status(json!(null)), None);
    assert_eq!(status(json!(3)), None);
    let without =
        parse_profile(&json!({"account":{"uuid":"u"},"organization":{"uuid":"o"}})).unwrap();
    assert_eq!(without.subscription_status, None);
    assert_eq!(without.identity.organization_id.as_deref(), Some("o"));
}
