use super::*;

#[cfg(not(windows))]
#[test]
fn unix_diagnostic_hints_do_not_talk_about_windows() {
    let hint = cli_hint(AgentId::Codex, "codex", None).expect("missing CLI gets a hint");
    for forbidden in ["Windows", "WSL", ".cmd", "Win32"] {
        assert!(!hint.contains(forbidden), "{hint}");
    }
    assert!(hint.contains("which codex"), "{hint}");
    assert_eq!(
        cli_hint(
            AgentId::Codex,
            "codex",
            Some(Path::new("/usr/local/bin/codex"))
        ),
        None
    );
    assert!(!home_missing_hint().contains("Windows"));
    assert!(!home_missing_hint().contains("WSL"));
    assert!(
        search_detail().starts_with("searched "),
        "{}",
        search_detail()
    );
}

#[test]
fn parse_defaults_when_missing_or_malformed() {
    assert_eq!(parse_settings(None), AppSettings::default());
    assert_eq!(parse_settings(Some("{nope")), AppSettings::default());
}

#[test]
fn saving_settings_replaces_the_complete_document() {
    let root = crate::paths::scratch_dir("settings-atomic-replace");
    let path = root.join("settings.json");
    let open_reader = root.join("open-reader.json");
    write_settings_document(&path, r#"{"generation":1}"#).unwrap();
    fs::hard_link(&path, &open_reader).unwrap();

    write_settings_document(&path, r#"{"generation":2}"#).unwrap();

    assert_eq!(
        fs::read_to_string(&open_reader).unwrap(),
        r#"{"generation":1}"#
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"generation":2}"#);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn parse_hidden_and_binary_paths() {
    let settings = parse_settings(Some(
        r#"{ "hiddenAgents": ["antigravity"], "binaryPaths": { "claude": "C:\\bin\\claude.cmd" } }"#,
    ));
    assert_eq!(settings.hidden_agents, vec![AgentId::Antigravity]);
    assert_eq!(
        settings
            .binary_paths
            .get(&AgentId::Claude)
            .map(String::as_str),
        Some(r"C:\bin\claude.cmd")
    );
}

#[test]
fn existing_settings_default_automatic_updates_to_enabled() {
    let new_settings = serde_json::to_value(parse_settings(None)).unwrap();
    let settings = parse_settings(Some(
        r#"{ "hiddenAgents": ["antigravity"], "binaryPaths": { "claude": "C:\\bin\\claude.cmd" } }"#,
    ));
    let serialized = serde_json::to_value(settings).unwrap();

    assert_eq!(new_settings["automaticUpdates"], true);
    assert_eq!(serialized["automaticUpdates"], true);
    assert_eq!(
        serialized["hiddenAgents"],
        serde_json::json!(["antigravity"])
    );
    assert_eq!(
        serialized["binaryPaths"]["claude"],
        serde_json::json!(r"C:\bin\claude.cmd")
    );
}

#[test]
fn existing_automatic_update_opt_out_is_preserved() {
    let settings = parse_settings(Some(r#"{ "automaticUpdates": false }"#));
    let serialized = serde_json::to_value(settings).unwrap();

    assert_eq!(serialized["automaticUpdates"], false);
}

#[test]
fn existing_settings_keep_limit_notifications_off_at_five_minutes() {
    let serialized =
        serde_json::to_value(parse_settings(Some(r#"{ "hiddenAgents": ["codex"] }"#))).unwrap();

    assert_eq!(serialized["limitNotifications"], false);
    assert_eq!(serialized["limitsPollMinutes"], 5);
}

#[test]
fn unsupported_limits_poll_interval_falls_back_to_five_minutes() {
    let serialized = serde_json::to_value(parse_settings(Some(
        r#"{ "limitNotifications": true, "limitsPollMinutes": 7 }"#,
    )))
    .unwrap();

    assert_eq!(serialized["limitNotifications"], true);
    assert_eq!(serialized["limitsPollMinutes"], 5);
}

#[test]
fn existing_settings_default_the_github_screen_off_at_sixty_seconds() {
    let serialized =
        serde_json::to_value(parse_settings(Some(r#"{ "hiddenAgents": ["codex"] }"#))).unwrap();

    assert_eq!(serialized["githubScopes"], serde_json::json!([]));
    assert_eq!(serialized["githubNotifications"], false);
    assert_eq!(serialized["githubPollSeconds"], 60);
}

#[test]
fn unsupported_github_poll_interval_falls_back_to_sixty_seconds() {
    let serialized = serde_json::to_value(parse_settings(Some(
        r#"{ "githubNotifications": true, "githubPollSeconds": 45 }"#,
    )))
    .unwrap();

    assert_eq!(serialized["githubNotifications"], true);
    assert_eq!(serialized["githubPollSeconds"], 60);
}

#[test]
fn github_scopes_are_normalised_to_search_qualifiers() {
    assert_eq!(
        normalize_github_scope(" foo/bar "),
        Some("repo:foo/bar".to_string())
    );
    assert_eq!(
        normalize_github_scope("repo:foo/bar.js"),
        Some("repo:foo/bar.js".to_string())
    );
    assert_eq!(
        normalize_github_scope("org:acme"),
        Some("org:acme".to_string())
    );
    assert_eq!(
        normalize_github_scope("user:me-1"),
        Some("user:me-1".to_string())
    );
    assert_eq!(
        normalize_github_scope("ORG:Acme"),
        Some("org:Acme".to_string())
    );
    for invalid in [
        "",
        "   ",
        "x",
        "org: x",
        "repo:o",
        "repo:o/r/x",
        "team:acme/core",
        "org:a b",
    ] {
        assert_eq!(normalize_github_scope(invalid), None, "{invalid:?}");
    }
}

#[test]
fn loading_settings_drops_malformed_github_scopes_and_normalises_the_rest() {
    let settings = parse_settings(Some(
        r#"{ "githubScopes": ["acme/app", "org: broken", "user:me", "user:me"] }"#,
    ));

    assert_eq!(
        settings.github_scopes,
        vec!["repo:acme/app".to_string(), "user:me".to_string()]
    );
}

#[test]
fn saving_settings_refuses_a_malformed_github_scope_and_names_it() {
    let err = save_settings(AppSettings {
        github_scopes: vec!["org:acme".into(), "org: broken".into()],
        ..AppSettings::default()
    })
    .unwrap_err();

    assert!(err.message.contains("org: broken"), "{}", err.message);
    assert!(err.message.contains("org:NAME"), "{}", err.message);
}

#[test]
fn cursor_uses_agent_as_its_command_and_keeps_the_legacy_alias() {
    assert_eq!(AgentId::Cursor.binary_name(), "agent");
    assert_eq!(agent_for_binary("agent"), Some(AgentId::Cursor));
    assert_eq!(agent_for_binary("cursor-agent"), Some(AgentId::Cursor));
    assert_eq!(agent_for_binary("cursor-agent.cmd"), Some(AgentId::Cursor));
    assert_eq!(
        agent_for_binary("cursor"),
        None,
        "the editor launcher is not the CLI"
    );
}

#[test]
fn missing_cursor_cli_hint_explains_the_agent_name_clash() {
    let hint = cli_hint(AgentId::Cursor, "agent", None).expect("hint");
    assert!(hint.contains("cursor-agent"), "{hint}");
    assert!(hint.contains("another product"), "{hint}");
    let other = cli_hint(AgentId::Codex, "codex", None).expect("hint");
    assert!(!other.contains("another product"), "{other}");
}

#[test]
fn refuses_hiding_every_provider() {
    let err = save_settings(AppSettings {
        hidden_agents: vec![
            AgentId::Claude,
            AgentId::Codex,
            AgentId::Antigravity,
            AgentId::Cursor,
        ],
        ..AppSettings::default()
    })
    .unwrap_err();
    assert!(err.message.contains("at least one provider"));
}
