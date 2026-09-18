use super::*;
use serde_json::json;

/// Sanitised `account/rateLimits/read` result from Codex app-server 0.148.0.
const APP_SERVER_CAPTURE: &str = r#"{
      "rateLimits": {
        "limitId": "codex", "limitName": null,
        "primary": {"usedPercent": 14, "windowDurationMins": 10080,
                    "resetsAt": 1787838960},
        "secondary": null,
        "credits": {"hasCredits": false, "unlimited": false, "balance": "0"},
        "planType": "pro"
      },
      "rateLimitsByLimitId": {
        "codex": {
          "limitId": "codex", "limitName": null,
          "primary": {"usedPercent": 14, "windowDurationMins": 10080,
                      "resetsAt": 1787838960},
          "secondary": null,
          "credits": {"hasCredits": false, "unlimited": false, "balance": "0"},
          "planType": "pro"
        },
        "codex_bengalfox": {
          "limitId": "codex_bengalfox", "limitName": "GPT-5.3-Codex-Spark",
          "primary": {"usedPercent": 0, "windowDurationMins": 300,
                      "resetsAt": 1787273137},
          "secondary": {"usedPercent": 6, "windowDurationMins": 10080,
                        "resetsAt": 1787859937},
          "credits": null,
          "planType": "pro"
        }
      },
      "rateLimitResetCredits": {"availableCount": 0, "credits": []}
    }"#;

#[test]
fn maps_app_server_buckets_without_duplicating_the_legacy_mirror() {
    let payload: RateLimitsResponse = serde_json::from_str(APP_SERVER_CAPTURE).unwrap();
    let parsed = parse_codex(&payload);

    assert_eq!(parsed.plan.as_deref(), Some("pro"));
    let windows: Vec<(&str, &str, LimitWindowKind, f64, Option<u64>)> = parsed
        .windows
        .iter()
        .map(|window| {
            (
                window.id.as_str(),
                window.label.as_str(),
                window.kind,
                window.used_percent,
                window.window_seconds,
            )
        })
        .collect();
    assert_eq!(
        windows,
        [
            (
                "primary",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                14.0,
                Some(604_800),
            ),
            (
                "extra:codex_bengalfox",
                "5 hour · GPT-5.3-Codex-Spark",
                LimitWindowKind::Model,
                0.0,
                Some(18_000),
            ),
            (
                "extra:codex_bengalfox:secondary",
                "Weekly · GPT-5.3-Codex-Spark",
                LimitWindowKind::Model,
                6.0,
                Some(604_800),
            ),
        ]
    );
    assert_eq!(parsed.credits, None);
}

#[test]
fn uses_the_single_bucket_fallback_and_maps_credits() {
    let payload: RateLimitsResponse = serde_json::from_value(json!({
        "rateLimits": {
            "limitId": "codex",
            "primary": {"usedPercent": 140, "windowDurationMins": 30},
            "secondary": null,
            "credits": {"hasCredits": true, "unlimited": false, "balance": "3"},
            "planType": "plus"
        },
        "rateLimitsByLimitId": null
    }))
    .unwrap();
    let parsed = parse_codex(&payload);

    assert_eq!(parsed.windows.len(), 1);
    assert_eq!(parsed.windows[0].used_percent, 100.0);
    assert_eq!(parsed.windows[0].label, "30 minute · all models");
    assert_eq!(
        parsed.credits,
        Some(LimitsCreditsDto {
            balance: "3".to_string(),
            unlimited: false,
        })
    );
}

#[test]
fn orders_weekly_before_session_even_when_app_server_returns_primary_first() {
    let payload: RateLimitsResponse = serde_json::from_value(json!({
        "rateLimits": {
            "limitId": "codex",
            "primary": {"usedPercent": 10, "windowDurationMins": 300},
            "secondary": {"usedPercent": 20, "windowDurationMins": 10080},
            "planType": "pro"
        }
    }))
    .unwrap();
    let parsed = parse_codex(&payload);

    let summary: Vec<(&str, LimitWindowKind)> = parsed
        .windows
        .iter()
        .map(|window| (window.id.as_str(), window.kind))
        .collect();
    assert_eq!(
        summary,
        [
            ("secondary", LimitWindowKind::Weekly),
            ("primary", LimitWindowKind::Session),
        ]
    );
}

fn expires(epoch: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(epoch, 0).map(|at| at.to_rfc3339())
}

#[test]
fn reset_credits_count_what_is_available_and_carry_the_soonest_expiry() {
    let payload: RateLimitsResponse = serde_json::from_value(json!({
        "rateLimits": {"limitId": "codex", "primary": {"usedPercent": 97, "windowDurationMins": 10080}},
        "rateLimitResetCredits": {"availableCount": 2, "credits": [
            {"id": "later", "resetType": "codexRateLimits", "status": "available",
             "grantedAt": 1787000000, "expiresAt": 1790000000, "title": null, "description": null},
            {"id": "spent", "resetType": "codexRateLimits", "status": "redeemed",
             "grantedAt": 1786000000, "expiresAt": 1788000000, "title": null, "description": null},
            {"id": "sooner", "resetType": "codexRateLimits", "status": "available",
             "grantedAt": 1787500000, "expiresAt": 1789000000, "title": null, "description": null},
            {"id": "forever", "resetType": "unknown", "status": "available",
             "grantedAt": 1787500000, "expiresAt": null, "title": null, "description": null},
            {"id": "garbled", "resetType": "codexRateLimits", "status": "available",
             "grantedAt": 1787500000, "expiresAt": i64::MIN, "title": null, "description": null}
        ]}
    }))
    .unwrap();

    assert_eq!(
        parse_codex(&payload).reset_credits,
        Some(LimitsResetCreditsDto {
            available_count: 2,
            // A redeemed credit's earlier expiry is not the next one to lapse, and an expiry that is
            // not a real instant does not hide the valid ones.
            next_expires_at: expires(1_789_000_000),
        })
    );
}

#[test]
fn reset_credits_tell_none_available_apart_from_a_cli_that_does_not_report_them() {
    let captured: RateLimitsResponse = serde_json::from_str(APP_SERVER_CAPTURE).unwrap();
    assert_eq!(
        parse_codex(&captured).reset_credits,
        Some(LimitsResetCreditsDto {
            available_count: 0,
            next_expires_at: None,
        })
    );

    // A read that skips the detail rows (`excludeResetCreditDetails`) still reports the count.
    let count_only: RateLimitsResponse = serde_json::from_value(json!({
        "rateLimits": {"limitId": "codex"},
        "rateLimitResetCredits": {"availableCount": 1, "credits": null}
    }))
    .unwrap();
    assert_eq!(
        parse_codex(&count_only).reset_credits,
        Some(LimitsResetCreditsDto {
            available_count: 1,
            next_expires_at: None,
        })
    );

    for unreported in [
        json!({"rateLimits": {"limitId": "codex"}}),
        json!({"rateLimits": {"limitId": "codex"}, "rateLimitResetCredits": null}),
    ] {
        let payload: RateLimitsResponse = serde_json::from_value(unreported).unwrap();
        assert_eq!(parse_codex(&payload).reset_credits, None);
    }
}

#[test]
fn a_paid_reset_offer_is_read_from_the_backend_banner_and_nothing_else_is() {
    let offered: RateLimitsResponse = serde_json::from_value(json!({
        "rateLimits": {"limitId": "codex", "primary": {"usedPercent": 100, "windowDurationMins": 10080}},
        "rateLimitUpsell": {
            "banner_type": "plus_rate_limit_reached",
            "title": "You have run out of Codex usage",
            "ctas": [
                {"action": "view_usage", "label": "See usage"},
                {"action": "buy_reset", "label": "Reset now",
                 "price": {"currency": "USD", "amount_minor_units": 800}}
            ]
        }
    }))
    .unwrap();
    assert_eq!(
        parse_codex(&offered).reset_offer,
        Some(LimitsResetOfferDto {
            currency: Some("USD".into()),
            amount_minor_units: Some(800),
        })
    );

    // A banner without the purchase call to action says nothing about buying one.
    let other: RateLimitsResponse = serde_json::from_value(json!({
        "rateLimits": {"limitId": "codex"},
        "rateLimitUpsell": {"banner_type": "usage_limit", "ctas": [{"action": "view_usage", "label": "See usage"}]}
    }))
    .unwrap();
    assert_eq!(parse_codex(&other).reset_offer, None);

    // The backend owns this blob, so a shape this build does not expect is not an offer, and a
    // priceless or garbled offer is still an offer.
    for (payload, expected) in [
        (json!({"rateLimits": {"limitId": "codex"}}), None),
        (
            json!({"rateLimits": {"limitId": "codex"}, "rateLimitUpsell": null}),
            None,
        ),
        (
            json!({"rateLimits": {"limitId": "codex"}, "rateLimitUpsell": {"ctas": "soon"}}),
            None,
        ),
        (
            json!({"rateLimits": {"limitId": "codex"}, "rateLimitUpsell": {"ctas": [{"action": "buy_reset"}]}}),
            Some(LimitsResetOfferDto {
                currency: None,
                amount_minor_units: None,
            }),
        ),
        (
            json!({"rateLimits": {"limitId": "codex"}, "rateLimitUpsell": {"ctas": [
                {"action": "buy_reset", "price": {"currency": "usd", "amount_minor_units": -1}}
            ]}}),
            Some(LimitsResetOfferDto {
                currency: Some("USD".into()),
                amount_minor_units: None,
            }),
        ),
    ] {
        let payload: RateLimitsResponse = serde_json::from_value(payload).unwrap();
        assert_eq!(parse_codex(&payload).reset_offer, expected);
    }
}
