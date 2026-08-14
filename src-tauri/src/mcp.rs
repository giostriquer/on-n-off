use std::collections::HashMap;

use serde::Deserialize;

use crate::dto::McpServerDto;

#[derive(Debug, Deserialize, Default)]
pub struct CodexMcpEntry {
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Default)]
struct ClaudeMcpEntry {
    #[serde(default)]
    r#type: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    url: Option<String>,
    #[serde(default)]
    disabled: bool,
}

fn claude_disabled_list(value: &serde_json::Value) -> Vec<String> {
    value
        .get("disabledMcpServers")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_claude_json(text: &str) -> Vec<McpServerDto> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(servers) = value.get("mcpServers").and_then(|value| value.as_object()) else {
        return Vec::new();
    };
    let disabled_list = claude_disabled_list(&value);
    let mut out = Vec::new();
    for (id, raw) in servers {
        let Ok(entry) = serde_json::from_value::<ClaudeMcpEntry>(raw.clone()) else {
            continue;
        };
        let enabled = !entry.disabled && !disabled_list.iter().any(|name| name == id);
        if let Some(dto) = mcp_dto(
            id,
            entry.r#type.as_deref(),
            entry.command.as_deref(),
            &entry.args,
            entry.url.as_deref(),
            enabled,
        ) {
            out.push(dto);
        }
    }
    out
}

pub fn parse_codex_map(entries: &HashMap<String, CodexMcpEntry>) -> Vec<McpServerDto> {
    let mut out = Vec::new();
    for (id, entry) in entries {
        if let Some(dto) = mcp_dto(
            id,
            entry.r#type.as_deref(),
            entry.command.as_deref(),
            &entry.args,
            entry.url.as_deref(),
            entry.enabled,
        ) {
            out.push(dto);
        }
    }
    out
}

fn mcp_dto(
    id: &str,
    type_hint: Option<&str>,
    command: Option<&str>,
    args: &[String],
    url: Option<&str>,
    enabled: bool,
) -> Option<McpServerDto> {
    let (system, source) = transport(type_hint, command, args, url)?;
    Some(McpServerDto {
        id: id.to_string(),
        name: id.to_string(),
        system: system.to_string(),
        source,
        enabled,
        togglable: true,
        origin: String::new(),
    })
}

#[derive(Debug, Deserialize, Default)]
struct AntigravityMcpEntry {
    #[serde(default)]
    r#type: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    url: Option<String>,
    #[serde(rename = "serverUrl")]
    server_url: Option<String>,
    #[serde(default)]
    disabled: bool,
}

pub fn parse_antigravity_json(text: &str) -> Vec<McpServerDto> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(servers) = value.get("mcpServers").and_then(|value| value.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (id, raw) in servers {
        let Ok(entry) = serde_json::from_value::<AntigravityMcpEntry>(raw.clone()) else {
            continue;
        };
        let url = entry
            .server_url
            .as_deref()
            .or(entry.url.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(dto) = mcp_dto(
            id,
            entry.r#type.as_deref(),
            entry.command.as_deref(),
            &entry.args,
            url,
            !entry.disabled,
        ) {
            out.push(dto);
        }
    }
    out
}

fn transport<'a>(
    type_hint: Option<&str>,
    command: Option<&str>,
    args: &[String],
    url: Option<&str>,
) -> Option<(&'a str, String)> {
    let hint = type_hint.map(str::trim).filter(|value| !value.is_empty());
    if let Some(url) = url.map(str::trim).filter(|value| !value.is_empty()) {
        let system = match hint {
            Some("sse") => "sse",
            Some("ws") => "ws",
            _ => "http",
        };
        return Some((system, url.to_string()));
    }
    let command = command.map(str::trim).filter(|value| !value.is_empty())?;
    let mut source = command.to_string();
    for arg in args {
        source.push(' ');
        source.push_str(arg);
    }
    Some(("stdio", source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_claude_user_scope_servers_and_skips_junk() {
        let servers = parse_claude_json(
            r#"{
                "numStartups": 3,
                "disabledMcpServers": ["docs"],
                "mcpServers": {
                    "github": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"], "disabled": true },
                    "docs": { "type": "http", "url": "https://docs.example/mcp" },
                    "broken": { "nope": true }
                },
                "projects": { "C:/tmp/app": { "mcpServers": { "local-only": { "command": "node" } } } }
            }"#,
        );
        let mut names: Vec<_> = servers.iter().map(|server| server.id.as_str()).collect();
        names.sort();
        assert_eq!(names, ["docs", "github"]);
        let github = servers.iter().find(|server| server.id == "github").unwrap();
        assert_eq!(github.system, "stdio");
        assert_eq!(github.source, "npx -y @modelcontextprotocol/server-github");
        assert!(!github.enabled);
        assert!(github.togglable);
        let docs = servers.iter().find(|server| server.id == "docs").unwrap();
        assert_eq!(docs.system, "http");
        assert_eq!(docs.source, "https://docs.example/mcp");
        assert!(!docs.enabled);
        assert!(docs.togglable);
    }

    #[test]
    fn malformed_or_missing_claude_json_is_empty() {
        assert!(parse_claude_json("{not json").is_empty());
        assert!(parse_claude_json("{}").is_empty());
    }

    #[test]
    fn reads_antigravity_server_url_and_disabled() {
        let servers = parse_antigravity_json(
            r#"{
                "mcpServers": {
                    "sqlite": { "command": "node", "args": ["server.js"], "disabled": true },
                    "remote": { "serverUrl": "https://api.example/mcp/" },
                    "broken": { "nope": true }
                }
            }"#,
        );
        let mut names: Vec<_> = servers.iter().map(|server| server.id.as_str()).collect();
        names.sort();
        assert_eq!(names, ["remote", "sqlite"]);
        let sqlite = servers.iter().find(|server| server.id == "sqlite").unwrap();
        assert!(!sqlite.enabled);
        assert_eq!(sqlite.system, "stdio");
        let remote = servers.iter().find(|server| server.id == "remote").unwrap();
        assert!(remote.enabled);
        assert_eq!(remote.system, "http");
        assert_eq!(remote.source, "https://api.example/mcp/");
    }

    #[test]
    fn reads_codex_enabled_flag_and_url_transport() {
        let mut entries = HashMap::new();
        entries.insert(
            "context7".into(),
            CodexMcpEntry {
                command: Some("npx".into()),
                args: vec!["-y".into(), "@upstash/context7-mcp".into()],
                url: None,
                r#type: None,
                enabled: true,
            },
        );
        entries.insert(
            "docs".into(),
            CodexMcpEntry {
                command: None,
                args: vec![],
                url: Some("https://docs.example/mcp".into()),
                r#type: Some("sse".into()),
                enabled: false,
            },
        );
        let servers = parse_codex_map(&entries);
        let context7 = servers
            .iter()
            .find(|server| server.id == "context7")
            .unwrap();
        assert_eq!(context7.system, "stdio");
        assert!(context7.enabled);
        let docs = servers.iter().find(|server| server.id == "docs").unwrap();
        assert_eq!(docs.system, "sse");
        assert!(!docs.enabled);
        assert!(docs.togglable);
        assert!(context7.togglable);
    }
}
