use std::path::{Path, PathBuf};

use crate::dto::AdapterError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallSource {
    GitUrl(String),
    GitHub {
        owner: String,
        repo: String,
        ref_name: Option<String>,
    },
    LocalDir(PathBuf),
    Plugin(String),
}

pub fn parse_install_source(input: &str) -> Result<InstallSource, AdapterError> {
    let value = input.trim();
    if value.is_empty() {
        return Err(AdapterError::message(INVALID));
    }
    if value.starts_with("git@") || value.to_ascii_lowercase().starts_with("ssh://") {
        return Err(AdapterError::message(INVALID));
    }
    if is_https_git(value) {
        return Ok(InstallSource::GitUrl(value.to_string()));
    }
    if looks_like_abs_path(value) {
        let path = PathBuf::from(value);
        if !path.is_dir() {
            return Err(AdapterError::message(format!("Folder not found: {value}")));
        }
        return Ok(InstallSource::LocalDir(path));
    }
    if let Some(id) = parse_plugin_id(value) {
        return Ok(InstallSource::Plugin(id));
    }
    if let Some((owner, repo, ref_name)) = parse_github(value) {
        return Ok(InstallSource::GitHub {
            owner,
            repo,
            ref_name,
        });
    }
    Err(AdapterError::message(INVALID))
}

impl InstallSource {
    pub fn as_cli_source(&self) -> String {
        match self {
            Self::GitUrl(url) => url.clone(),
            Self::GitHub {
                owner,
                repo,
                ref_name: Some(ref_name),
            } => format!("{owner}/{repo}@{ref_name}"),
            Self::GitHub {
                owner,
                repo,
                ref_name: None,
            } => format!("{owner}/{repo}"),
            Self::LocalDir(path) => path.display().to_string(),
            Self::Plugin(id) => id.clone(),
        }
    }

    pub fn claude_install_argv(&self) -> Vec<String> {
        match self {
            Self::Plugin(id) => vec![
                "plugin".into(),
                "install".into(),
                "-s".into(),
                "user".into(),
                "-y".into(),
                id.clone(),
            ],
            _ => vec![
                "plugin".into(),
                "marketplace".into(),
                "add".into(),
                "--scope".into(),
                "user".into(),
                self.as_cli_source(),
            ],
        }
    }

    pub fn claude_uninstall_argv(plugin_id: &str) -> Vec<String> {
        vec![
            "plugin".into(),
            "uninstall".into(),
            "-s".into(),
            "user".into(),
            "-y".into(),
            plugin_id.to_string(),
        ]
    }

    pub fn codex_install_argv(&self) -> Vec<String> {
        match self {
            Self::Plugin(id) => vec!["plugin".into(), "add".into(), "--json".into(), id.clone()],
            Self::GitHub {
                owner,
                repo,
                ref_name: Some(ref_name),
            } => vec![
                "plugin".into(),
                "marketplace".into(),
                "add".into(),
                "--json".into(),
                format!("{owner}/{repo}"),
                "--ref".into(),
                ref_name.clone(),
            ],
            _ => vec![
                "plugin".into(),
                "marketplace".into(),
                "add".into(),
                "--json".into(),
                self.as_cli_source(),
            ],
        }
    }

    pub fn codex_uninstall_argv(plugin_id: &str) -> Vec<String> {
        vec![
            "plugin".into(),
            "remove".into(),
            "--json".into(),
            plugin_id.to_string(),
        ]
    }
}

const INVALID: &str = "Use an HTTPS git URL or owner/repo.";

fn is_https_git(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("https://") && value.contains('/') && !value.chars().any(char::is_whitespace)
}

fn looks_like_abs_path(value: &str) -> bool {
    Path::new(value).is_absolute()
        || (value.len() >= 3
            && value.as_bytes()[0].is_ascii_alphabetic()
            && value.as_bytes()[1] == b':'
            && (value.as_bytes()[2] == b'\\' || value.as_bytes()[2] == b'/'))
}

fn parse_plugin_id(value: &str) -> Option<String> {
    let (name, source) = value.split_once('@')?;
    if name.is_empty()
        || source.is_empty()
        || name.contains('/')
        || source.contains('/')
        || name.contains('\\')
        || source.contains('\\')
        || value.chars().any(char::is_whitespace)
    {
        return None;
    }
    Some(value.to_string())
}

fn parse_github(value: &str) -> Option<(String, String, Option<String>)> {
    let (base, ref_name) = match value.split_once('@') {
        Some((base, ref_name)) if !ref_name.is_empty() => (base, Some(ref_name.to_string())),
        Some(_) => return None,
        None => (value, None),
    };
    let (owner, repo) = base.split_once('/')?;
    if owner.is_empty()
        || repo.is_empty()
        || owner.contains('/')
        || repo.contains('/')
        || owner.contains('\\')
        || repo.contains('\\')
    {
        return None;
    }
    Some((owner.to_string(), repo.to_string(), ref_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_git_url_github_shorthand_and_plugin_id() {
        assert_eq!(
            parse_install_source("https://github.com/acme/tools.git").unwrap(),
            InstallSource::GitUrl("https://github.com/acme/tools.git".into())
        );
        assert_eq!(
            parse_install_source("acme/tools").unwrap(),
            InstallSource::GitHub {
                owner: "acme".into(),
                repo: "tools".into(),
                ref_name: None,
            }
        );
        assert_eq!(
            parse_install_source("acme/tools@v1").unwrap(),
            InstallSource::GitHub {
                owner: "acme".into(),
                repo: "tools".into(),
                ref_name: Some("v1".into()),
            }
        );
        assert_eq!(
            parse_install_source("workbench@workshop").unwrap(),
            InstallSource::Plugin("workbench@workshop".into())
        );
    }

    #[test]
    fn rejects_ssh_garbage_and_missing_folder() {
        assert!(parse_install_source("").is_err());
        assert!(parse_install_source("git@github.com:acme/tools.git").is_err());
        assert!(parse_install_source("not a source").is_err());
        let missing = crate::paths::scratch_dir("on-n-off-install-missing").join("nope");
        assert!(parse_install_source(&missing.display().to_string()).is_err());
    }

    #[test]
    fn accepts_existing_local_folder() {
        let dir = crate::paths::scratch_dir("on-n-off-install-dir");
        let parsed = parse_install_source(&dir.display().to_string()).unwrap();
        assert_eq!(parsed, InstallSource::LocalDir(dir));
    }

    #[test]
    fn claude_and_codex_argv_match_help() {
        let plugin = InstallSource::Plugin("workbench@workshop".into());
        assert_eq!(
            plugin.claude_install_argv(),
            ["plugin", "install", "-s", "user", "-y", "workbench@workshop"]
        );
        assert_eq!(
            plugin.codex_install_argv(),
            ["plugin", "add", "--json", "workbench@workshop"]
        );
        let git = InstallSource::GitHub {
            owner: "acme".into(),
            repo: "tools".into(),
            ref_name: Some("main".into()),
        };
        assert_eq!(
            git.claude_install_argv(),
            ["plugin", "marketplace", "add", "--scope", "user", "acme/tools@main"]
        );
        assert_eq!(
            git.codex_install_argv(),
            ["plugin", "marketplace", "add", "--json", "acme/tools", "--ref", "main"]
        );
        assert_eq!(
            InstallSource::claude_uninstall_argv("workbench@workshop"),
            ["plugin", "uninstall", "-s", "user", "-y", "workbench@workshop"]
        );
        assert_eq!(
            InstallSource::codex_uninstall_argv("workbench@workshop"),
            ["plugin", "remove", "--json", "workbench@workshop"]
        );
    }
}
