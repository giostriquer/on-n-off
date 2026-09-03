use std::env;
use std::fs;

use serde::{Deserialize, Serialize};

use crate::paths;

/// Resolved feature flags. Unknown file keys are ignored. Defaults are off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureFlags {
    pub master_cut: bool,
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
pub fn resolve_flags(
    file_json: Option<&str>,
    env_lookup: impl Fn(&str) -> Option<String>,
) -> FeatureFlags {
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
    resolve_flags(json.as_deref(), |key| {
        env::var(format!("ON_N_OFF_FLAG_{key}")).ok()
    })
}

#[cfg(test)]
mod tests;
