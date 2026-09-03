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
    NpxSkills {
        source: String,
        skill: Option<String>,
    },
}

pub fn parse_install_source(input: &str) -> Result<InstallSource, AdapterError> {
    let value = input.trim();
    if value.is_empty() {
        return Err(AdapterError::message(INVALID));
    }
    if value.starts_with("git@") || value.to_ascii_lowercase().starts_with("ssh://") {
        return Err(AdapterError::message(INVALID));
    }
    if let Some(parsed) = parse_npx_skills(value) {
        return parsed;
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
            Self::NpxSkills { source, skill } => match skill {
                Some(skill) => format!("{source} --skill {skill}"),
                None => source.clone(),
            },
        }
    }

    pub fn is_npx_skills(&self) -> bool {
        matches!(self, Self::NpxSkills { .. })
    }

    pub fn npx_skills_argv(&self, agent: &str) -> Vec<String> {
        let Self::NpxSkills { source, skill } = self else {
            return Vec::new();
        };
        let mut args = vec![
            "-y".into(),
            "skills".into(),
            "add".into(),
            source.clone(),
            "-g".into(),
            "-y".into(),
            "-a".into(),
            agent.to_string(),
        ];
        if let Some(skill) = skill {
            args.push("--skill".into());
            args.push(skill.clone());
        }
        args
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
            Self::NpxSkills { .. } => self.npx_skills_argv("claude-code"),
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
            Self::NpxSkills { .. } => self.npx_skills_argv("codex"),
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

    pub fn agy_install_argv(&self) -> Vec<String> {
        match self {
            Self::NpxSkills { .. } => self.npx_skills_argv("antigravity"),
            _ => vec!["plugin".into(), "install".into(), self.as_cli_source()],
        }
    }

    pub fn agy_uninstall_argv(plugin_id: &str) -> Vec<String> {
        let (name, _) = crate::paths::plugin_id_parts(plugin_id);
        vec!["plugin".into(), "uninstall".into(), name]
    }

    pub fn plugin_marketplace(plugin_id: &str) -> Option<&str> {
        plugin_id
            .split_once('@')
            .map(|(_, source)| source)
            .filter(|source| !source.is_empty() && !source.eq_ignore_ascii_case("local"))
    }

    pub fn claude_marketplace_update_argv(marketplace: &str) -> Vec<String> {
        vec![
            "plugin".into(),
            "marketplace".into(),
            "update".into(),
            marketplace.to_string(),
        ]
    }

    pub fn claude_update_argv(plugin_id: &str) -> Vec<String> {
        vec![
            "plugin".into(),
            "update".into(),
            "-s".into(),
            "user".into(),
            "-y".into(),
            plugin_id.to_string(),
        ]
    }

    pub fn codex_marketplace_upgrade_argv(marketplace: &str) -> Vec<String> {
        vec![
            "plugin".into(),
            "marketplace".into(),
            "upgrade".into(),
            "--json".into(),
            marketplace.to_string(),
        ]
    }

    pub fn codex_update_argv(plugin_id: &str) -> Vec<String> {
        vec![
            "plugin".into(),
            "add".into(),
            "--json".into(),
            plugin_id.to_string(),
        ]
    }
}

const INVALID: &str = "Use an HTTPS git URL, owner/repo, name@marketplace, or npx skills add.";

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

fn parse_npx_skills(value: &str) -> Option<Result<InstallSource, AdapterError>> {
    let tokens = tokenize(value);
    let first = tokens.first()?;
    let npx = first.eq_ignore_ascii_case("npx");
    if !npx && !first.eq_ignore_ascii_case("skills") {
        return None;
    }
    let mut index = if npx { 1 } else { 0 };
    if npx {
        while index < tokens.len() && is_npx_prefix_flag(&tokens[index]) {
            index += 1;
        }
    }
    if index >= tokens.len() || !tokens[index].eq_ignore_ascii_case("skills") {
        return Some(Err(AdapterError::message(INVALID)));
    }
    index += 1;
    if index >= tokens.len() || !tokens[index].eq_ignore_ascii_case("add") {
        return Some(Err(AdapterError::message(INVALID)));
    }
    index += 1;
    let mut source = None;
    let mut skill = None;
    while index < tokens.len() {
        let token = &tokens[index];
        if token == "--skill" || token == "-s" {
            index += 1;
            skill = tokens.get(index).cloned();
        } else if token == "-a" || token == "--agent" || token == "--agents" {
            index += 1;
        } else if !is_skills_ignore_flag(token) && !token.starts_with('-') && source.is_none() {
            source = Some(token.clone());
        }
        index += 1;
    }
    let Some(source) = source else {
        return Some(Err(AdapterError::message(INVALID)));
    };
    if source.starts_with("git@") || source.to_ascii_lowercase().starts_with("ssh://") {
        return Some(Err(AdapterError::message(INVALID)));
    }
    Some(Ok(InstallSource::NpxSkills { source, skill }))
}

fn tokenize(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for ch in value.chars() {
        if let Some(mark) = quote {
            if ch == mark {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            continue;
        }
        if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn is_npx_prefix_flag(token: &str) -> bool {
    matches!(token, "-y" | "--yes" | "-q" | "--quiet")
}

fn is_skills_ignore_flag(token: &str) -> bool {
    matches!(
        token,
        "-g" | "--global" | "-y" | "--yes" | "--all" | "--copy" | "-l" | "--list"
    )
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
mod tests;
