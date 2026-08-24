use std::env;
use std::path::PathBuf;

use crate::dto::AdapterError;

pub fn user_home() -> Result<PathBuf, AdapterError> {
    if let Ok(root) = env::var("ON_N_OFF_HOME") {
        return Ok(PathBuf::from(root));
    }
    env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .map(PathBuf::from)
        .map_err(|_| AdapterError::message("home directory not found"))
}

pub fn claude_root() -> Result<PathBuf, AdapterError> {
    Ok(user_home()?.join(".claude"))
}

pub fn codex_root() -> Result<PathBuf, AdapterError> {
    Ok(user_home()?.join(".codex"))
}

pub fn agents_skills_root() -> Result<PathBuf, AdapterError> {
    Ok(user_home()?.join(".agents").join("skills"))
}

pub fn gemini_root() -> Result<PathBuf, AdapterError> {
    Ok(user_home()?.join(".gemini"))
}

pub fn cursor_root() -> Result<PathBuf, AdapterError> {
    Ok(user_home()?.join(".cursor"))
}

pub fn antigravity_cli_root() -> Result<PathBuf, AdapterError> {
    Ok(gemini_root()?.join("antigravity-cli"))
}

pub fn antigravity_config_plugins() -> Result<PathBuf, AdapterError> {
    Ok(gemini_root()?.join("config").join("plugins"))
}

pub fn antigravity_cli_plugins() -> Result<PathBuf, AdapterError> {
    Ok(antigravity_cli_root()?.join("plugins"))
}

pub fn antigravity_mcp_config() -> Result<PathBuf, AdapterError> {
    Ok(gemini_root()?.join("config").join("mcp_config.json"))
}

pub fn antigravity_cli_skills() -> Result<PathBuf, AdapterError> {
    Ok(antigravity_cli_root()?.join("skills"))
}

pub fn plugin_id_parts(id: &str) -> (String, String) {
    match id.split_once('@') {
        Some((name, source)) => (name.to_string(), source.to_string()),
        None => (id.to_string(), "local".to_string()),
    }
}

pub fn backup_root() -> Result<PathBuf, AdapterError> {
    Ok(user_home()?.join(".on-n-off").join("backups"))
}

pub fn flags_path_for(home: &std::path::Path) -> PathBuf {
    home.join(".on-n-off").join("flags.json")
}

pub fn flags_path() -> Result<PathBuf, AdapterError> {
    Ok(flags_path_for(&user_home()?))
}

pub fn settings_path_for(home: &std::path::Path) -> PathBuf {
    home.join(".on-n-off").join("settings.json")
}

pub fn settings_path() -> Result<PathBuf, AdapterError> {
    Ok(settings_path_for(&user_home()?))
}

pub fn limits_monitor_state_path_for(home: &std::path::Path) -> PathBuf {
    home.join(".on-n-off").join("limits-monitor.json")
}

pub fn limits_monitor_state_path() -> Result<PathBuf, AdapterError> {
    Ok(limits_monitor_state_path_for(&user_home()?))
}

/// Last successful pull-request read, so the GitHub screen has something to show at launch.
pub fn github_prs_path_for(home: &std::path::Path) -> PathBuf {
    home.join(".on-n-off").join("github").join("prs.json")
}

/// The CI monitor's last-seen rollup per own pull request, so a restart never re-notifies.
pub fn github_monitor_state_path_for(home: &std::path::Path) -> PathBuf {
    home.join(".on-n-off").join("github").join("monitor.json")
}

pub fn github_monitor_state_path() -> Result<PathBuf, AdapterError> {
    Ok(github_monitor_state_path_for(&user_home()?))
}

pub fn installed_items_path_for(home: &std::path::Path) -> PathBuf {
    home.join(".on-n-off").join("installed-items.json")
}

pub fn installed_items_path() -> Result<PathBuf, AdapterError> {
    Ok(installed_items_path_for(&user_home()?))
}

pub fn normalize_skill_path(path: &str) -> String {
    let mut normalized = path.replace('/', "\\").to_lowercase();
    if !normalized.ends_with("\\skill.md") {
        normalized = format!("{}\\skill.md", normalized.trim_end_matches('\\'));
    }
    normalized
}

pub fn newest_dir(parent: &std::path::Path) -> Option<PathBuf> {
    if !parent.is_dir() {
        return None;
    }
    let mut dirs: Vec<_> = std::fs::read_dir(parent)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    if dirs.is_empty() {
        return None;
    }
    if dirs.len() == 1 {
        return dirs.pop();
    }
    dirs.into_iter().max_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
    })
}

/// Fresh per-call fixture directory. The counter keeps parallel tests apart even when the
/// clock has coarse (microsecond) resolution, as it does on macOS.
#[cfg(test)]
pub fn scratch_dir(prefix: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "{prefix}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn flags_path_lives_under_on_n_off_home() {
        let home = std::env::temp_dir().join("scratch");
        assert_eq!(
            flags_path_for(&home).strip_prefix(&home),
            Ok(Path::new(".on-n-off").join("flags.json").as_path())
        );
        assert_eq!(
            settings_path_for(&home).strip_prefix(&home),
            Ok(Path::new(".on-n-off").join("settings.json").as_path())
        );
        assert_eq!(
            limits_monitor_state_path_for(&home).strip_prefix(&home),
            Ok(Path::new(".on-n-off").join("limits-monitor.json").as_path())
        );
        assert_eq!(
            github_prs_path_for(&home).strip_prefix(&home),
            Ok(Path::new(".on-n-off")
                .join("github")
                .join("prs.json")
                .as_path())
        );
        assert_eq!(
            github_monitor_state_path_for(&home).strip_prefix(&home),
            Ok(Path::new(".on-n-off")
                .join("github")
                .join("monitor.json")
                .as_path())
        );
    }

    #[test]
    fn normalize_skill_path_unifies_slash_and_skill_md() {
        assert_eq!(
            normalize_skill_path(r"C:\Users\Me\.agents\skills\loom-feed"),
            r"c:\users\me\.agents\skills\loom-feed\skill.md"
        );
        assert_eq!(
            normalize_skill_path("C:/Users/Me/.agents/skills/loom-feed/SKILL.md"),
            r"c:\users\me\.agents\skills\loom-feed\skill.md"
        );
    }
}
