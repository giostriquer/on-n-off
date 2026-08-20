//! Read-only Claude quota observations from Desktop's local plan-usage history.
//!
//! Desktop owns its authenticated session. We do not read its cookies or Keychain secret. The
//! history file contains only sampled percentages and organization ids, so it is safe to merge as
//! another dated observation of the same account and quota windows.

use std::fs;
use std::path::{Path, PathBuf};

#[cfg(target_os = "windows")]
use std::env;

#[cfg(test)]
use chrono::SecondsFormat;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::json::{percent, window};
use crate::dto::{LimitWindowDto, LimitWindowKind};

const FILE_NAME: &str = "plan-usage-history.json";
const SUPPORTED_VERSION: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    MacOs,
    Windows,
    Other,
}

pub(super) struct DesktopUsage {
    pub observed_at: DateTime<Utc>,
    pub windows: Vec<LimitWindowDto>,
}

/// Resolve Claude Desktop's data file. `ON_N_OFF_HOME` keeps tests and fixture QA under the
/// disposable home; a normal Windows run respects a redirected `%APPDATA%`.
pub(super) fn history_path(home: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    let app_data = env::var_os("APPDATA")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let app_data: Option<PathBuf> = None;
    #[cfg(target_os = "windows")]
    let use_home = env::var_os("ON_N_OFF_HOME").is_some();
    #[cfg(not(target_os = "windows"))]
    let use_home = false;
    resolve_history_path(home, app_data.as_deref(), use_home, current_platform())
}

#[cfg(test)]
pub(super) fn history_path_for_home(home: &Path) -> PathBuf {
    resolve_history_path(home, None, true, current_platform())
}

fn resolve_history_path(
    home: &Path,
    app_data: Option<&Path>,
    use_home: bool,
    platform: Platform,
) -> PathBuf {
    let data_dir = match platform {
        Platform::MacOs => home
            .join("Library")
            .join("Application Support")
            .join("Claude"),
        Platform::Windows => {
            let root = if use_home {
                home.join("AppData").join("Roaming")
            } else {
                app_data
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| home.join("AppData").join("Roaming"))
            };
            root.join("Claude")
        }
        Platform::Other => home.join("Claude"),
    };
    data_dir.join(FILE_NAME)
}

const fn current_platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Other
    }
}

pub(super) fn read_latest(path: &Path, organization_id: &str) -> Option<DesktopUsage> {
    let raw = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    if value.get("version").and_then(Value::as_u64) != Some(SUPPORTED_VERSION) {
        return None;
    }

    value
        .get("samples")?
        .as_array()?
        .iter()
        .filter(|sample| sample.get("org").and_then(Value::as_str) == Some(organization_id))
        .filter_map(parse_sample)
        .max_by_key(|(timestamp_ms, _)| *timestamp_ms)
        .and_then(|(timestamp_ms, windows)| {
            Some(DesktopUsage {
                observed_at: DateTime::<Utc>::from_timestamp_millis(timestamp_ms)?,
                windows,
            })
        })
}

fn parse_sample(sample: &Value) -> Option<(i64, Vec<LimitWindowDto>)> {
    let timestamp_ms = sample.get("t").and_then(Value::as_i64)?;
    DateTime::<Utc>::from_timestamp_millis(timestamp_ms)?;
    let usage = sample.get("u")?;
    let mut windows = Vec::new();
    if let Some(used) = percent(usage.get("sd")) {
        windows.push(window(
            "weekly_all",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            used,
            None,
        ));
    }
    if let Some(used) = percent(usage.get("fh")) {
        windows.push(window(
            "session",
            "5 hour · all models",
            LimitWindowKind::Session,
            used,
            None,
        ));
    }
    (!windows.is_empty()).then_some((timestamp_ms, windows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::scratch_dir;

    fn write_history(home: &Path, body: &str) -> PathBuf {
        let path = history_path_for_home(home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn latest_sample_is_selected_only_by_exact_organization_id() {
        let home = scratch_dir("claude-desktop-history");
        let path = write_history(
            &home,
            r#"{"version":2,"samples":[
                {"t":1000,"org":"org-1","u":{"fh":1,"sd":2}},
                {"t":3000,"org":"org-2","u":{"fh":90,"sd":91}},
                {"t":2000,"org":"org-1","u":{"fh":3,"sd":4,"unknown":99}}
            ]}"#,
        );

        let usage = read_latest(&path, "org-1").unwrap();
        assert_eq!(
            usage
                .observed_at
                .to_rfc3339_opts(SecondsFormat::Millis, true),
            "1970-01-01T00:00:02.000Z"
        );
        assert_eq!(
            usage
                .windows
                .iter()
                .map(|window| (window.id.as_str(), window.used_percent))
                .collect::<Vec<_>>(),
            [("weekly_all", 4.0), ("session", 3.0)]
        );
        assert!(usage
            .windows
            .iter()
            .all(|window| window.resets_at.is_none()));
        assert!(read_latest(&path, "ORG-1").is_none());
    }

    #[test]
    fn malformed_unknown_version_and_unrecognized_usage_are_ignored() {
        let home = scratch_dir("claude-desktop-history");
        let path = write_history(&home, "{broken");
        assert!(read_latest(&path, "org-1").is_none());

        fs::write(
            &path,
            r#"{"version":3,"samples":[{"t":1000,"org":"org-1","u":{"fh":5}}]}"#,
        )
        .unwrap();
        assert!(read_latest(&path, "org-1").is_none());

        fs::write(
            &path,
            r#"{"version":2,"samples":[{"t":1000,"org":"org-1","u":{"other":5}}]}"#,
        )
        .unwrap();
        assert!(read_latest(&path, "org-1").is_none());
    }

    #[test]
    fn platform_paths_use_the_exact_desktop_data_locations() {
        let home = Path::new("fixture-home");
        let app_data = Path::new("redirected-app-data");
        assert_eq!(
            resolve_history_path(home, None, true, Platform::MacOs),
            home.join("Library")
                .join("Application Support")
                .join("Claude")
                .join(FILE_NAME)
        );
        assert_eq!(
            resolve_history_path(home, Some(app_data), false, Platform::Windows),
            app_data.join("Claude").join(FILE_NAME)
        );
        assert_eq!(
            resolve_history_path(home, Some(app_data), true, Platform::Windows),
            home.join("AppData")
                .join("Roaming")
                .join("Claude")
                .join(FILE_NAME)
        );
        assert_eq!(
            resolve_history_path(home, None, false, Platform::Windows),
            home.join("AppData")
                .join("Roaming")
                .join("Claude")
                .join(FILE_NAME)
        );
    }
}
