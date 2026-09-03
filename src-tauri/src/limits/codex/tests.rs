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
