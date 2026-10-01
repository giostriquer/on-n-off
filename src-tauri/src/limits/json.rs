use serde_json::Value;

use crate::dto::{LimitWindowDto, LimitWindowKind};

pub(crate) fn percent(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(0.0, 100.0))
}

pub(crate) fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub(crate) fn humanize(raw: &str) -> String {
    raw.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn window(
    id: impl Into<String>,
    label: impl Into<String>,
    kind: LimitWindowKind,
    used_percent: f64,
    resets_at: Option<String>,
) -> LimitWindowDto {
    LimitWindowDto {
        id: id.into(),
        label: label.into(),
        kind,
        used_percent,
        resets_at,
        window_seconds: None,
        observed_at: String::new(),
    }
}
