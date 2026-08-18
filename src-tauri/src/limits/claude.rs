//! Claude subscription limits: parse `GET api.anthropic.com/api/oauth/usage` (pure).
//!
//! The payload carries a normalized `limits[]` array (kind/group/percent/resets_at) plus the
//! older top-level `five_hour` / `seven_day` / `seven_day_<model>` objects. The array wins when
//! it has usable entries; the legacy keys are the fallback.

use serde_json::Value;

use super::json::{humanize, optional_string, percent, window};
use crate::dto::{LimitWindowDto, LimitWindowKind};

pub fn parse_claude(payload: &Value) -> Vec<LimitWindowDto> {
    let normalized: Vec<LimitWindowDto> = payload
        .get("limits")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(normalized_window).collect())
        .unwrap_or_default();
    if !normalized.is_empty() {
        return normalized;
    }
    legacy_windows(payload)
}

fn normalized_window(entry: &Value) -> Option<LimitWindowDto> {
    let kind = optional_string(entry.get("kind"))?;
    let group = optional_string(entry.get("group"))?;
    let used = percent(entry.get("percent"))?;
    let resets_at = optional_string(entry.get("resets_at"));
    let scope = scope_name(entry.get("scope"));
    let (window_kind, label) = match (group.as_str(), kind.as_str()) {
        ("session", _) => (LimitWindowKind::Session, "5 hour · all models".to_string()),
        ("weekly", "weekly_all") => (LimitWindowKind::Weekly, "Weekly · all models".to_string()),
        ("weekly", other) => {
            let name = match &scope {
                Some(scope) => scope.clone(),
                None => humanize(other.strip_prefix("weekly_").unwrap_or(other)),
            };
            (LimitWindowKind::Model, format!("Weekly · {name}"))
        }
        _ => return None,
    };
    let id = match &scope {
        Some(scope) => format!("{kind}:{scope}"),
        None => kind,
    };
    Some(window(id, label, window_kind, used, resets_at))
}

/// `scope.model.display_name`, else `scope.surface`, for per-model / per-surface windows.
fn scope_name(scope: Option<&Value>) -> Option<String> {
    let scope = scope?;
    optional_string(
        scope
            .get("model")
            .and_then(|model| model.get("display_name")),
    )
    .or_else(|| optional_string(scope.get("surface")))
}

fn legacy_windows(payload: &Value) -> Vec<LimitWindowDto> {
    const LEGACY: [(&str, &str, &str, LimitWindowKind); 4] = [
        (
            "five_hour",
            "session",
            "5 hour · all models",
            LimitWindowKind::Session,
        ),
        (
            "seven_day",
            "weekly_all",
            "Weekly · all models",
            LimitWindowKind::Weekly,
        ),
        (
            "seven_day_opus",
            "weekly_opus",
            "Weekly · Opus",
            LimitWindowKind::Model,
        ),
        (
            "seven_day_sonnet",
            "weekly_sonnet",
            "Weekly · Sonnet",
            LimitWindowKind::Model,
        ),
    ];
    LEGACY
        .iter()
        .filter_map(|(key, id, label, kind)| {
            let entry = payload.get(*key)?;
            let used = percent(entry.get("utilization"))?;
            let resets_at = optional_string(entry.get("resets_at"));
            Some(window(*id, *label, *kind, used, resets_at))
        })
        .collect()
}

#[cfg(test)]
mod tests {
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
}
