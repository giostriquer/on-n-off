use std::env;
use std::path::PathBuf;

use crate::dto::{AdapterError, AgentId, AgentInfo};

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

pub fn binary_on_path(name: &str) -> bool {
    resolve_cli_binary(name).is_some()
}

/// First existing CLI file: explicit path, settings override, PATH, then well-known dirs.
/// On Windows, PATHEXT launchers (`.cmd` / `.exe`) win over extensionless npm/nvm shims.
pub fn resolve_cli_binary(name: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(name);
    if let Some(path) = windows_launchable(&candidate) {
        return Some(path);
    }
    if let Some(override_path) = crate::settings::binary_override_for(name) {
        if let Some(path) = windows_launchable(&override_path) {
            return Some(path);
        }
    }
    find_on_path(name).or_else(|| find_in_dirs(name, &well_known_cli_dirs()))
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    find_in_dirs(name, &env::split_paths(&path).collect::<Vec<_>>())
}

fn pathext() -> Vec<String> {
    if cfg!(windows) {
        env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .map(|ext| ext.trim_start_matches('.').to_string())
            .filter(|ext| !ext.is_empty())
            .collect()
    } else {
        Vec::new()
    }
}

fn has_windows_launcher_ext(path: &std::path::Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    pathext()
        .iter()
        .any(|known| known.eq_ignore_ascii_case(ext))
}

/// Prefer a Win32 launcher next to an extensionless nvm/npm shim.
fn windows_launchable(path: &std::path::Path) -> Option<PathBuf> {
    if !path.is_file() {
        if cfg!(windows) && path.extension().is_none() {
            for ext in pathext() {
                let mut with_ext = path.to_path_buf();
                with_ext.set_extension(ext);
                if with_ext.is_file() {
                    return Some(with_ext);
                }
            }
        }
        return None;
    }
    if !cfg!(windows) {
        return Some(path.to_path_buf());
    }
    if has_windows_launcher_ext(path) {
        return Some(path.to_path_buf());
    }
    for ext in pathext() {
        let mut with_ext = path.to_path_buf();
        with_ext.set_extension(ext);
        if with_ext.is_file() {
            return Some(with_ext);
        }
    }
    None
}

pub(crate) fn find_in_dirs(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    for dir in dirs {
        if let Some(path) = windows_launchable(&dir.join(name)) {
            return Some(path);
        }
    }
    None
}

pub fn well_known_cli_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = user_home() {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join("AppData").join("Roaming").join("npm"));
        dirs.push(home.join("AppData").join("Local").join("Volta").join("bin"));
    }
    if let Ok(appdata) = env::var("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("npm"));
    }
    if let Ok(local) = env::var("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("Volta").join("bin"));
    }
    if cfg!(windows) {
        dirs.push(PathBuf::from(r"C:\nvm4w\nodejs"));
    }
    dirs
}

pub fn plugin_id_parts(id: &str) -> (String, String) {
    match id.split_once('@') {
        Some((name, source)) => (name.to_string(), source.to_string()),
        None => (id.to_string(), "local".to_string()),
    }
}

pub fn agent_info(id: AgentId, binary: &str) -> AgentInfo {
    let resolved = resolve_cli_binary(binary);
    let cli_ok = resolved.is_some();
    AgentInfo {
        id,
        display_name: id.display_name().to_string(),
        cli_ok,
        cli_error: if cli_ok {
            None
        } else {
            Some(format!("{} CLI not found.", id.display_name()))
        },
        install_git: cli_ok,
        install_folder: cli_ok,
        plugin_toggle: match id {
            AgentId::Claude => cli_ok,
            AgentId::Codex => true,
            AgentId::Antigravity => cli_ok,
        },
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

#[cfg(test)]
pub fn scratch_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_path_lives_under_on_n_off_home() {
        let path = flags_path_for(&PathBuf::from(r"C:\scratch"));
        assert_eq!(path, PathBuf::from(r"C:\scratch\.on-n-off\flags.json"));
        assert_eq!(
            settings_path_for(&PathBuf::from(r"C:\scratch")),
            PathBuf::from(r"C:\scratch\.on-n-off\settings.json")
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

    #[cfg(windows)]
    #[test]
    fn windows_prefers_cmd_over_extensionless_nvm_shim() {
        let dir = scratch_dir("on-n-off-cli-shim");
        std::fs::write(dir.join("claude"), "#!/usr/bin/env node\n").unwrap();
        std::fs::write(dir.join("claude.cmd"), "@echo off\r\n").unwrap();
        let found = find_in_dirs("claude", std::slice::from_ref(&dir)).expect("cmd launcher");
        let name = found.file_name().unwrap().to_string_lossy();
        assert!(
            name.eq_ignore_ascii_case("claude.cmd"),
            "expected claude.cmd, got {name}"
        );
    }
}
