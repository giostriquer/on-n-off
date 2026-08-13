use std::env;
use std::fs;

use serde::{Deserialize, Serialize};

use crate::paths;

/// Resolved feature flags. Unknown file keys are ignored. Defaults are off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureFlags {
    pub master_cut: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self { master_cut: false }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FlagOverlay {
    master_cut: Option<bool>,
}

pub fn parse_env_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" => Some(false),
        _ => None,
    }
}

/// File overlay first, then `ON_N_OFF_FLAG_*` env. Invalid env values are ignored.
pub fn resolve_flags(file_json: Option<&str>, env_lookup: impl Fn(&str) -> Option<String>) -> FeatureFlags {
    let mut flags = FeatureFlags::default();
    if let Some(json) = file_json {
        if let Ok(overlay) = serde_json::from_str::<FlagOverlay>(json) {
            if let Some(value) = overlay.master_cut {
                flags.master_cut = value;
            }
        }
    }
    if let Some(raw) = env_lookup("MASTER_CUT") {
        if let Some(value) = parse_env_bool(&raw) {
            flags.master_cut = value;
        }
    }
    flags
}

pub fn load_flags() -> FeatureFlags {
    let json = paths::flags_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok());
    resolve_flags(json.as_deref(), |key| env::var(format!("ON_N_OFF_FLAG_{key}")).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_master_cut_off() {
        let flags = resolve_flags(None, |_| None);
        assert_eq!(flags, FeatureFlags { master_cut: false });
    }

    #[test]
    fn file_can_enable_master_cut() {
        let flags = resolve_flags(Some(r#"{ "masterCut": true, "nope": 1 }"#), |_| None);
        assert!(flags.master_cut);
    }

    #[test]
    fn malformed_file_falls_back_to_defaults() {
        let flags = resolve_flags(Some("{not json"), |_| None);
        assert!(!flags.master_cut);
    }

    #[test]
    fn env_overrides_file() {
        let flags = resolve_flags(Some(r#"{ "masterCut": true }"#), |key| {
            (key == "MASTER_CUT").then(|| "0".into())
        });
        assert!(!flags.master_cut);
    }

    #[test]
    fn env_parses_common_bool_tokens() {
        assert_eq!(parse_env_bool("TRUE"), Some(true));
        assert_eq!(parse_env_bool(" yes "), Some(true));
        assert_eq!(parse_env_bool("off"), Some(false));
        assert_eq!(parse_env_bool("maybe"), None);
    }
}
