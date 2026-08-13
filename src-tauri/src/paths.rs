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

pub fn binary_on_path(name: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    let exts = if cfg!(windows) {
        env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
    } else {
        String::new()
    };
    env::split_paths(&path).any(|dir| {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return true;
        }
        if cfg!(windows) {
            return exts.split(';').any(|ext| {
                let mut exe = candidate.clone();
                exe.set_extension(ext.trim_start_matches('.'));
                exe.is_file()
            });
        }
        false
    })
}

pub fn plugin_id_parts(id: &str) -> (String, String) {
    match id.split_once('@') {
        Some((name, source)) => (name.to_string(), source.to_string()),
        None => (id.to_string(), "local".to_string()),
    }
}

pub fn agent_info(id: AgentId, binary: &str) -> AgentInfo {
    let cli_ok = binary_on_path(binary);
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
        },
    }
}

pub fn backup_root() -> Result<PathBuf, AdapterError> {
    Ok(user_home()?.join(".on-n-off").join("backups"))
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
