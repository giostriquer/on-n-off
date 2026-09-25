use super::*;
use crate::dto::LimitsWorkspaceCreditsDto;
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
    // The Spark bucket is one of Codex's hidden ones: no surface shows it (see below).
    assert_eq!(
        windows,
        [(
            "primary",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            14.0,
            Some(604_800),
        )]
    );
    assert_eq!(parsed.credits, None);
}

/// A reading of the main weekly window beside one extra bucket per `(id, name)`, each with a
/// primary and a secondary window.
fn with_extra_buckets(buckets: &[(&str, &str)]) -> Reading {
    let mut by_id = serde_json::Map::new();
    by_id.insert(
        "codex".into(),
        json!({"limitId": "codex", "primary": {"usedPercent": 14, "windowDurationMins": 10080}}),
    );
    for (id, name) in buckets {
        by_id.insert(
            (*id).into(),
            json!({"limitId": id, "limitName": name,
                "primary": {"usedPercent": 1, "windowDurationMins": 300},
                "secondary": {"usedPercent": 2, "windowDurationMins": 10080}}),
        );
    }
    let payload: RateLimitsResponse = serde_json::from_value(json!({
        "rateLimits": {"limitId": "codex", "primary": {"usedPercent": 14, "windowDurationMins": 10080}},
        "rateLimitsByLimitId": by_id
    }))
    .unwrap();
    parse_codex(&payload)
}

fn ids(reading: &Reading) -> Vec<&str> {
    reading
        .windows
        .iter()
        .map(|window| window.id.as_str())
        .collect()
}

/// Codex's internal buckets are dropped by id, whatever they are named: both windows of each.
/// Only those exact buckets: one whose id merely starts with the same letters stays.
#[test]
fn the_reader_drops_codexs_internal_buckets_by_id_whatever_their_name() {
    let reading = with_extra_buckets(&[
        ("base_model_inference", "Inference"),
        ("codex_bengalfox", "Bengal preview"),
        ("codex_bengalfox_next", "Next"),
    ]);
    assert_eq!(
        ids(&reading),
        [
            "primary",
            "extra:codex_bengalfox_next",
            "extra:codex_bengalfox_next:secondary"
        ]
    );
}

/// The reserve and Spark buckets are dropped by the model name their label ends with, whatever
/// their id, in any case and spacing. A longer name that only ends with a hidden one stays, and so
/// does a hidden name that is not the last part of the label.
#[test]
fn the_reader_drops_the_reserve_and_spark_buckets_by_name_whatever_their_id() {
    let reading = with_extra_buckets(&[
        ("reserve", "GPT-Reserve"),
        ("spark", "  gpt-5.3-codex-SPARK "),
        ("team_reserve", "Team GPT-Reserve"),
        ("reserve_team", "GPT-Reserve · Team"),
    ]);
    assert_eq!(
        ids(&reading),
        [
            "primary",
            "extra:reserve_team",
            "extra:reserve_team:secondary",
            "extra:team_reserve",
            "extra:team_reserve:secondary"
        ]
    );
}

/// Every other extra limit stays, with its own name.
#[test]
fn the_reader_keeps_every_other_codex_model_limit() {
    let reading = with_extra_buckets(&[("gpt_luna", "GPT-5.6-Luna")]);
    let labels: Vec<&str> = reading
        .windows
        .iter()
        .map(|window| window.label.as_str())
        .collect();
    assert_eq!(
        labels,
        [
            "Weekly · all models",
            "5 hour · GPT-5.6-Luna",
            "Weekly · GPT-5.6-Luna"
        ]
    );
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
    let card = crate::limits::pipeline::finish(
        crate::dto::AgentId::Codex,
        crate::dto::LimitsStatus::Ok,
        None,
        crate::limits::Parsed {
            account: None,
            reading: parse_codex(&payload),
        },
    );

    let summary: Vec<(&str, LimitWindowKind)> = card
        .reading
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
    // The banner reaches clients as the backend wrote it: `rate_limit_upsell` is an untyped value
    // on the app-server response, so its nested keys keep the backend's snake_case.
    let offered: RateLimitsResponse = serde_json::from_value(json!({
        "rateLimits": {"limitId": "codex", "primary": {"usedPercent": 100, "windowDurationMins": 10080}},
        "rateLimitUpsell": {
            "banner_type": "plus_rate_limit_reached",
            "title": "You have run out of Codex usage",
            "ctas": [
                {"action": "view_usage", "label": "See usage"},
                {"action": "buy_reset", "label": "Reset now",
                 "price": {"currency": "usd", "amount_minor_units": 800}}
            ]
        }
    }))
    .unwrap();
    assert_eq!(
        parse_codex(&offered).reset_offer,
        Some(LimitsResetOfferDto {
            price: Some(LimitsPriceDto {
                amount_minor_units: 800,
                // Upper-cased once, here, so no other layer has to.
                currency: "USD".into(),
            }),
        })
    );

    // A banner without the purchase call to action says nothing about buying one.
    let other: RateLimitsResponse = serde_json::from_value(json!({
        "rateLimits": {"limitId": "codex"},
        "rateLimitUpsell": {"banner_type": "usage_limit", "ctas": [{"action": "view_usage", "label": "See usage"}]}
    }))
    .unwrap();
    assert_eq!(parse_codex(&other).reset_offer, None);
}

#[test]
fn an_offer_survives_a_price_this_app_will_not_show() {
    let priced = |price: serde_json::Value| {
        let payload: RateLimitsResponse = serde_json::from_value(json!({
            "rateLimits": {"limitId": "codex"},
            "rateLimitUpsell": {"ctas": [{"action": "buy_reset", "label": "Reset now", "price": price}]}
        }))
        .unwrap();
        parse_codex(&payload).reset_offer
    };
    let priceless = Some(LimitsResetOfferDto { price: None });

    assert_eq!(
        priced(json!({"currency": "  eur  ", "amount_minor_units": 500})),
        Some(LimitsResetOfferDto {
            price: Some(LimitsPriceDto {
                amount_minor_units: 500,
                currency: "EUR".into()
            }),
        })
    );
    // Not an ISO 4217 code, so there is nothing to print the number beside.
    assert_eq!(
        priced(json!({"currency": "US", "amount_minor_units": 800})),
        priceless
    );
    assert_eq!(
        priced(json!({"currency": "US DOLLARS", "amount_minor_units": 800})),
        priceless
    );
    assert_eq!(
        priced(json!({"currency": "US$", "amount_minor_units": 800})),
        priceless
    );
    // An amount that is not a whole count of minor units, or beyond any reset ever sold.
    assert_eq!(
        priced(json!({"currency": "USD", "amount_minor_units": 800.5})),
        priceless
    );
    assert_eq!(
        priced(json!({"currency": "USD", "amount_minor_units": "800"})),
        priceless
    );
    assert_eq!(
        priced(json!({"currency": "USD", "amount_minor_units": -1})),
        priceless
    );
    assert_eq!(
        priced(json!({"currency": "USD", "amount_minor_units": u64::MAX})),
        priceless
    );
    assert_eq!(priced(json!({"currency": "USD"})), priceless);
    assert_eq!(priced(json!(null)), priceless);

    // Whole yen is a whole count of minor units, and stays one.
    assert_eq!(
        priced(json!({"currency": "JPY", "amount_minor_units": 1200})),
        Some(LimitsResetOfferDto {
            price: Some(LimitsPriceDto {
                amount_minor_units: 1200,
                currency: "JPY".into()
            }),
        })
    );
}

#[test]
fn a_banner_shaped_unlike_the_one_this_app_knows_offers_nothing() {
    for banner in [
        json!(null),
        json!({"ctas": "soon"}),
        json!({"banner_type": "usage_limit"}),
        json!({"ctas": [{"label": "Reset now"}]}),
        json!({"ctas": [42]}),
    ] {
        let payload: RateLimitsResponse = serde_json::from_value(json!({
            "rateLimits": {"limitId": "codex"}, "rateLimitUpsell": banner
        }))
        .unwrap();
        assert_eq!(parse_codex(&payload).reset_offer, None);
    }
    let absent: RateLimitsResponse =
        serde_json::from_value(json!({"rateLimits": {"limitId": "codex"}})).unwrap();
    assert_eq!(parse_codex(&absent).reset_offer, None);
}

/// A business workspace pools its credits; each member's share of them is Codex's spend control,
/// which app-server reports on the main bucket as `individualLimit` and `spendControlReached`.
/// App-server's answer for a business member, shaped like `APP_SERVER_CAPTURE`: the main bucket both
/// on its own and under its id, which is the copy `parse_codex` reads.
fn business_payload(
    individual_limit: serde_json::Value,
    reached: serde_json::Value,
) -> RateLimitsResponse {
    let main = json!({
        "limitId": "codex",
        "primary": {"usedPercent": 12, "windowDurationMins": 300},
        "secondary": null,
        "credits": {"hasCredits": true, "unlimited": false, "balance": "0"},
        "individualLimit": individual_limit,
        "spendControlReached": reached,
        "planType": "self_serve_business_prolite"
    });
    serde_json::from_value(json!({"rateLimits": main, "rateLimitsByLimitId": {"codex": main}}))
        .unwrap()
}

#[test]
fn maps_a_business_members_share_of_the_workspace_credits() {
    let payload = business_payload(
        json!({"limit": "25000", "used": "8000", "remainingPercent": 68, "resetsAt": 1790000000}),
        json!(false),
    );

    assert_eq!(
        parse_codex(&payload).workspace_credits,
        Some(LimitsWorkspaceCreditsDto {
            limit: "25000".to_string(),
            used: "8000".to_string(),
            used_percent: 32.0,
            resets_at: Some("2026-09-21T14:13:20+00:00".to_string()),
            reached: false,
        })
    );
}

#[test]
fn a_share_used_up_is_marked_reached() {
    let payload = business_payload(
        json!({"limit": "25000", "used": "25000", "remainingPercent": 0, "resetsAt": 1790000000}),
        json!(true),
    );

    let share = parse_codex(&payload).workspace_credits.unwrap();
    assert!(share.reached);
}

/// The meter is Codex's own: what its status line shows as used is 100 less what the backend says
/// remains, which can differ from the amounts' ratio by the backend's rounding.
#[test]
fn a_shares_meter_is_what_codex_says_remains() {
    let payload = business_payload(
        json!({"limit": "25000", "used": "8123", "remainingPercent": 68}),
        json!(false),
    );

    assert_eq!(
        parse_codex(&payload)
            .workspace_credits
            .unwrap()
            .used_percent,
        32.0
    );
}

/// A share the backend says is used up is full, whatever it says remains.
#[test]
fn a_reached_share_is_all_used() {
    let payload = business_payload(
        json!({"limit": "25000", "used": "9000", "remainingPercent": 64}),
        json!(true),
    );

    assert_eq!(
        parse_codex(&payload)
            .workspace_credits
            .unwrap()
            .used_percent,
        100.0
    );
}

/// Without a usable remaining percent, the meter is the amounts' ratio: all of a share of
/// nothing, and never more than all of it.
#[test]
fn without_what_remains_the_meter_is_what_is_used_of_the_limit() {
    for (share, expected) in [
        (json!({"limit": "25000", "used": "8000"}), 32.0),
        (
            json!({"limit": "25000", "used": "8000", "remainingPercent": 150}),
            32.0,
        ),
        (
            json!({"limit": "25000", "used": "8000", "remainingPercent": -5}),
            32.0,
        ),
        (
            json!({"limit": "25000", "used": "8000", "remainingPercent": "68"}),
            32.0,
        ),
        (json!({"limit": "0", "used": "0"}), 100.0),
        (json!({"limit": "100", "used": "120"}), 100.0),
        (json!({"limit": "25000", "used": "0"}), 0.0),
    ] {
        let payload = business_payload(share.clone(), json!(false));
        assert_eq!(
            parse_codex(&payload)
                .workspace_credits
                .unwrap()
                .used_percent,
            expected,
            "{share}"
        );
    }
}

/// Right after a reset a member has used nothing, which is still a share to show.
#[test]
fn a_share_with_nothing_used_yet_reads() {
    let payload = business_payload(json!({"limit": "25000", "used": "0"}), json!(false));

    let share = parse_codex(&payload).workspace_credits.unwrap();
    assert_eq!((share.limit.as_str(), share.used.as_str()), ("25000", "0"));
}

/// Amounts arrive as strings; a number is read the same way.
#[test]
fn a_share_given_in_numbers_still_reads() {
    let payload = business_payload(
        json!({"limit": 25000.5, "used": 8000, "resetsAt": null}),
        json!(null),
    );

    let share = parse_codex(&payload).workspace_credits.unwrap();
    assert_eq!(
        (share.limit.as_str(), share.used.as_str()),
        ("25000.5", "8000")
    );
    assert_eq!(share.resets_at, None);
    assert!(!share.reached);
}

/// An amount that is not a finite number of at least zero leaves nothing to show, as Codex's own
/// status line treats it.
#[test]
fn a_share_whose_amounts_are_not_counts_is_not_shown() {
    for (limit, used) in [
        (json!("n/a"), json!("8000")),
        (json!("25000"), json!("")),
        (json!("25000"), json!("-1")),
        (json!("NaN"), json!("0")),
        (json!("inf"), json!("0")),
        (json!(true), json!("0")),
    ] {
        let payload = business_payload(json!({"limit": limit, "used": used}), json!(false));
        assert_eq!(
            parse_codex(&payload).workspace_credits,
            None,
            "{limit} / {used}"
        );
    }
}

/// Without an individual limit there is no share to show, whatever else the bucket says.
#[test]
fn no_share_without_an_individual_limit() {
    let plain: RateLimitsResponse = serde_json::from_str(APP_SERVER_CAPTURE).unwrap();
    let reached_only = business_payload(json!(null), json!(true));
    let unreadable = business_payload(json!({"limit": "25000"}), json!(false));

    assert_eq!(parse_codex(&plain).workspace_credits, None);
    assert_eq!(parse_codex(&reached_only).workspace_credits, None);
    assert_eq!(parse_codex(&unreadable).workspace_credits, None);
}
