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
mod tests;
