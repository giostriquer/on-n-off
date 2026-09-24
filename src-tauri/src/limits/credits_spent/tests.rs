use super::*;
use serde_json::json;

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 24).unwrap()
}

/// A breakdown in the shape the endpoint answers: one row per day, with the credits each model used.
fn breakdown(days: &[(&str, &[f64])]) -> Value {
    json!({
        "data": days.iter().map(|(date, credits)| json!({
            "date": date,
            "product_surface_usage_values": {"cli": 1.0, "vscode": 0.0},
            "premium_usage_values": {"total_usage_credits": {}, "credit_usage_credits": {}},
            "models": credits.iter().enumerate().map(|(i, credits)| json!({
                "model": format!("model-{i}"),
                "credits": credits,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "units": "credits",
        "data_freshness_ts": "2026-09-24T19:00:00Z",
        "group_by": "day",
    })
}

/// Codex's own parser sums each day's per-model credits, and the days in the window.
#[test]
fn sums_each_days_models_over_the_last_7_and_30_days() {
    let payload = breakdown(&[
        ("2026-08-26", &[100.0]),
        ("2026-09-17", &[50.0, 25.5]),
        ("2026-09-18", &[1000.0]),
        ("2026-09-24T00:00:00Z", &[200.25, 0.25]),
    ]);

    assert_eq!(
        parse(&payload, today()),
        Some(LimitsCreditsSpentDto {
            last_7_days: 1200.5,
            last_30_days: 1376.0,
            updated_at: Some("2026-09-24T19:00:00Z".to_string()),
        })
    );
}

#[test]
fn days_outside_the_30_day_window_are_left_out() {
    let payload = breakdown(&[
        ("2026-08-25", &[999.0]),
        ("2026-08-26", &[1.0]),
        ("2026-09-25", &[999.0]),
    ]);

    let spent = parse(&payload, today()).unwrap();
    assert_eq!((spent.last_7_days, spent.last_30_days), (0.0, 1.0));
}

/// A member who has used nothing yet has still been answered: nothing spent is a figure.
#[test]
fn no_rows_is_nothing_spent() {
    let spent = parse(&breakdown(&[]), today()).unwrap();
    assert_eq!((spent.last_7_days, spent.last_30_days), (0.0, 0.0));
}

/// Anything counted in another unit (the app also reads token breakdowns) is not credits.
#[test]
fn only_a_breakdown_counted_in_credits_is_read() {
    let mut tokens = breakdown(&[("2026-09-24", &[5.0])]);
    tokens["units"] = json!("tokens");
    let mut unitless = breakdown(&[("2026-09-24", &[5.0])]);
    unitless.as_object_mut().unwrap().remove("units");

    assert_eq!(parse(&tokens, today()), None);
    assert_eq!(parse(&unitless, today()), None);
    assert_eq!(parse(&json!({"units": "credits"}), today()), None);
}

#[test]
fn an_amount_that_is_not_a_count_of_credits_adds_nothing() {
    let mut payload = breakdown(&[("2026-09-24", &[10.0])]);
    payload["data"][0]["models"]
        .as_array_mut()
        .unwrap()
        .extend([json!({"credits": -5.0}), json!({"credits": "7"}), json!({})]);
    payload["data"].as_array_mut().unwrap().extend([
        json!({"date": "not a date", "models": [{"credits": 3.0}]}),
        json!({}),
    ]);

    let spent = parse(&payload, today()).unwrap();
    assert_eq!((spent.last_7_days, spent.last_30_days), (10.0, 10.0));
}

#[test]
fn a_missing_or_unreadable_freshness_is_no_update_time() {
    let mut missing = breakdown(&[]);
    missing.as_object_mut().unwrap().remove("data_freshness_ts");
    let mut unreadable = breakdown(&[]);
    unreadable["data_freshness_ts"] = json!("yesterday");

    assert_eq!(parse(&missing, today()).unwrap().updated_at, None);
    assert_eq!(parse(&unreadable, today()).unwrap().updated_at, None);
}

/// The app asks for whole UTC days ending today: `start = today - (days - 1)`.
#[test]
fn asks_for_the_30_utc_days_ending_today_by_day() {
    assert_eq!(
        query_url("https://example.test/breakdown", today()),
        "https://example.test/breakdown?start_date=2026-08-26&end_date=2026-09-24&group_by=day"
    );
}

/// Codex's own `PlanType::is_workspace_account` (openai/codex rust-v0.156.1,
/// `codex-rs/protocol/src/account.rs`): team-like, business-like, education-like and enterprise.
#[test]
fn workspace_plans_are_the_ones_codex_counts_as_workspace_accounts() {
    for plan in [
        "team",
        "self_serve_business_prolite",
        "self_serve_business_usage_based",
        "business",
        "ent26",
        "enterprise_cbp_automation",
        "enterprise_cbp_usage_based",
        "enterprise",
        "edu",
        "edu_plus",
        "edu_pro",
    ] {
        assert!(is_workspace_plan(plan), "{plan}");
    }
    for plan in [
        "free", "go", "plus", "pro", "prolite", "guest", "", "unknown",
    ] {
        assert!(!is_workspace_plan(plan), "{plan}");
    }
}
