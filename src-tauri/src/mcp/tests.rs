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
