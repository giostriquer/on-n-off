//! Codex subscription limits: parse `GET chatgpt.com/backend-api/wham/usage` (pure).
//!
//! `rate_limit.primary_window` / `secondary_window` are the plan-wide windows (weekly vs
//! session decided by `limit_window_seconds`); `additional_rate_limits[]` are per-model
//! limits; `credits` is the extra-usage balance. Of the identity fields only `email` is read,
//! separately, as the account label.

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::json::{optional_string, percent, window};
use super::Parsed;
use crate::dto::{LimitWindowDto, LimitWindowKind, LimitsCreditsDto};

const WEEKLY_THRESHOLD_SECONDS: u64 = 24 * 60 * 60;

pub(super) fn parse_codex(payload: &Value) -> Parsed {
    let mut windows = Vec::new();
    if let Some(rate_limit) = payload.get("rate_limit") {
        windows.extend(plan_window(rate_limit, "primary_window", "primary"));
        windows.extend(plan_window(rate_limit, "secondary_window", "secondary"));
    }
    if let Some(extra) = payload
        .get("additional_rate_limits")
        .and_then(Value::as_array)
    {
        windows.extend(
            extra
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| extra_window(index, entry)),
        );
    }
    Parsed {
        account: None,
        plan: optional_string(payload.get("plan_type")),
        windows,
        credits: credits(payload.get("credits")),
    }
}

/// The signed-in account's email, used only as the on-screen label for its card.
pub(super) fn account_email(payload: &Value) -> Option<String> {
    optional_string(payload.get("email"))
}

fn plan_window(rate_limit: &Value, key: &str, id: &str) -> Option<LimitWindowDto> {
    let entry = rate_limit.get(key)?;
    let (kind, used, resets_at) = window_parts(entry)?;
    let label = match kind {
        LimitWindowKind::Session => format!("{} · all models", session_length(entry)),
        _ => "Weekly · all models".to_string(),
    };
    Some(window(id, label, kind, used, resets_at))
}

/// "5 hour" / "30 minute" from `limit_window_seconds`, for the short rolling window's label.
fn session_length(entry: &Value) -> String {
    let seconds = entry
        .get("limit_window_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if seconds >= 3600 && seconds.is_multiple_of(3600) {
        format!("{} hour", seconds / 3600)
    } else if seconds >= 60 {
        format!("{} minute", seconds / 60)
    } else {
        "Session".to_string()
    }
}

/// Per-model limits render as compact `Model` rows; the window length only picks the prefix.
fn extra_window(index: usize, entry: &Value) -> Option<LimitWindowDto> {
    let window_entry = entry.get("rate_limit")?.get("primary_window")?;
    let (kind, used, resets_at) = window_parts(window_entry)?;
    let name = optional_string(entry.get("limit_name"));
    let id = optional_string(entry.get("metered_feature"))
        .or_else(|| name.clone())
        .unwrap_or_else(|| index.to_string());
    let prefix = match kind {
        LimitWindowKind::Session => session_length(window_entry),
        _ => "Weekly".to_string(),
    };
    let label = format!("{prefix} · {}", name.as_deref().unwrap_or("extra limit"));
    Some(window(
        format!("extra:{id}"),
        label,
        LimitWindowKind::Model,
        used,
        resets_at,
    ))
}

fn window_parts(entry: &Value) -> Option<(LimitWindowKind, f64, Option<String>)> {
    let used = percent(entry.get("used_percent"))?;
    let seconds = entry.get("limit_window_seconds").and_then(Value::as_u64);
    let kind = match seconds {
        Some(s) if s < WEEKLY_THRESHOLD_SECONDS => LimitWindowKind::Session,
        _ => LimitWindowKind::Weekly,
    };
    let resets_at = entry
        .get("reset_at")
        .and_then(Value::as_i64)
        .and_then(|epoch| DateTime::<Utc>::from_timestamp(epoch, 0))
        .map(|at| at.to_rfc3339());
    Some((kind, used, resets_at))
}

fn credits(value: Option<&Value>) -> Option<LimitsCreditsDto> {
    let credits = value?;
    if !credits
        .get("has_credits")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    Some(LimitsCreditsDto {
        balance: optional_string(credits.get("balance")).unwrap_or_else(|| "0".to_string()),
        unlimited: credits
            .get("unlimited")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Sanitised capture from 2026-08-17 (Pro plan): identity fields removed.
    const CAPTURED: &str = r#"{
      "plan_type": "pro",
      "rate_limit": {
        "allowed": true, "limit_reached": false,
        "primary_window": {"used_percent": 1, "limit_window_seconds": 604800,
                           "reset_after_seconds": 599727, "reset_at": 1787614473},
        "secondary_window": null
      },
      "code_review_rate_limit": null,
      "additional_rate_limits": [
        {"limit_name": "GPT-5.3-Codex-Spark", "metered_feature": "codex_bengalfox",
         "rate_limit": {"allowed": true, "limit_reached": false,
                        "primary_window": {"used_percent": 0, "limit_window_seconds": 604800,
                                           "reset_after_seconds": 604800, "reset_at": 1787619547},
                        "secondary_window": null}}
      ],
      "credits": {"has_credits": false, "unlimited": false, "overage_limit_reached": false,
                  "balance": "0", "approx_local_messages": [0], "approx_cloud_messages": [0]},
      "spend_control": {"reached": false, "individual_limit": null},
      "rate_limit_reached_type": null,
      "promo": null
    }"#;

    #[test]
    fn maps_the_captured_payload_to_plan_windows_and_no_credits() {
        let payload: serde_json::Value = serde_json::from_str(CAPTURED).unwrap();
        let parsed = parse_codex(&payload);
        assert_eq!(parsed.plan.as_deref(), Some("pro"));
        assert_eq!(parsed.credits, None);
        assert_eq!(parsed.windows.len(), 2);
        assert_eq!(parsed.windows[0].id, "primary");
        assert_eq!(parsed.windows[0].kind, LimitWindowKind::Weekly);
        assert_eq!(parsed.windows[0].label, "Weekly · all models");
        assert_eq!(parsed.windows[0].used_percent, 1.0);
        assert_eq!(
            parsed.windows[0].resets_at.as_deref(),
            Some("2026-08-24T23:34:33+00:00")
        );
        assert_eq!(parsed.windows[1].id, "extra:codex_bengalfox");
        assert_eq!(parsed.windows[1].kind, LimitWindowKind::Model);
        assert_eq!(parsed.windows[1].label, "Weekly · GPT-5.3-Codex-Spark");
        assert_eq!(parsed.windows[1].used_percent, 0.0);
    }

    #[test]
    fn the_account_email_is_read_separately_and_never_lands_in_parsed_output() {
        let payload = json!({"email": " me@example.com ", "plan_type": "pro"});
        assert_eq!(account_email(&payload).as_deref(), Some("me@example.com"));
        assert_eq!(account_email(&json!({})), None);
        assert_eq!(parse_codex(&payload).account, None);
    }

    #[test]
    fn short_windows_are_sessions_and_long_ones_weekly() {
        let payload = json!({
            "rate_limit": {
                "primary_window": {"used_percent": 42, "limit_window_seconds": 18000, "reset_at": 1787020000},
                "secondary_window": {"used_percent": 9.5, "limit_window_seconds": 604800, "reset_at": 1787600000}
            }
        });
        let parsed = parse_codex(&payload);
        assert_eq!(parsed.plan, None);
        assert_eq!(parsed.windows[0].id, "primary");
        assert_eq!(parsed.windows[0].kind, LimitWindowKind::Session);
        assert_eq!(parsed.windows[0].label, "5 hour · all models");
        assert_eq!(parsed.windows[1].id, "secondary");
        assert_eq!(parsed.windows[1].kind, LimitWindowKind::Weekly);
        assert_eq!(parsed.windows[1].used_percent, 9.5);
    }

    #[test]
    fn session_labels_follow_the_window_length() {
        for (seconds, label) in [
            (18000, "5 hour · all models"),
            (1800, "30 minute · all models"),
            (5400, "90 minute · all models"),
            (30, "Session · all models"),
        ] {
            let payload = json!({
                "rate_limit": {"primary_window": {"used_percent": 1, "limit_window_seconds": seconds}}
            });
            assert_eq!(parse_codex(&payload).windows[0].label, label, "{seconds}s");
        }
    }

    #[test]
    fn a_full_day_window_or_an_unknown_length_counts_as_weekly() {
        let payload = json!({
            "rate_limit": {
                "primary_window": {"used_percent": 1, "limit_window_seconds": 86400},
                "secondary_window": {"used_percent": 2}
            }
        });
        let parsed = parse_codex(&payload);
        assert_eq!(parsed.windows[0].kind, LimitWindowKind::Weekly);
        assert_eq!(parsed.windows[1].kind, LimitWindowKind::Weekly);
    }

    #[test]
    fn credits_are_reported_only_when_the_account_has_some() {
        let payload = json!({
            "credits": {"has_credits": true, "unlimited": false, "balance": "12.5"}
        });
        let parsed = parse_codex(&payload);
        assert_eq!(
            parsed.credits,
            Some(crate::dto::LimitsCreditsDto {
                balance: "12.5".to_string(),
                unlimited: false
            })
        );
        assert!(parsed.windows.is_empty());
    }

    #[test]
    fn extra_limits_fall_back_to_the_limit_name_for_ids_and_skip_broken_entries() {
        let payload = json!({
            "additional_rate_limits": [
                {"limit_name": "Spark", "rate_limit": {"primary_window": {"used_percent": 5, "limit_window_seconds": 604800}}},
                {"limit_name": "Broken", "rate_limit": {"primary_window": {"used_percent": "n/a"}}},
                {"rate_limit": {"primary_window": {"used_percent": 250, "limit_window_seconds": 100}}},
                42
            ]
        });
        let parsed = parse_codex(&payload);
        let summary: Vec<(&str, &str, f64)> = parsed
            .windows
            .iter()
            .map(|w| (w.id.as_str(), w.label.as_str(), w.used_percent))
            .collect();
        assert_eq!(
            summary,
            [
                ("extra:Spark", "Weekly · Spark", 5.0),
                ("extra:2", "1 minute · extra limit", 100.0)
            ]
        );
        assert_eq!(parsed.windows[0].resets_at, None);
    }
}
