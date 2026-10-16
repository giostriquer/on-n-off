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

fn save_under(home: &Path, settings: AppSettings) -> Result<AppSettings, AdapterError> {
    save_settings_to(settings, || Ok(crate::paths::settings_path_for(home)))
}

fn refused_save(settings: AppSettings) -> AdapterError {
    save_settings_to(settings, || {
        panic!("a refused save must not resolve a settings path")
    })
    .unwrap_err()
}

#[test]
fn saving_settings_refuses_a_malformed_github_scope_and_names_it() {
    let err = refused_save(AppSettings {
        github_scopes: vec!["org:acme".into(), "org: broken".into()],
        ..AppSettings::default()
    });

    assert!(err.message.contains("org: broken"), "{}", err.message);
    assert!(err.message.contains("org:NAME"), "{}", err.message);
}

#[test]
fn a_saved_document_names_every_provider_by_its_lowercase_key_in_tab_order() {
    let home = crate::paths::scratch_dir("settings-provider-keys");
    save_under(
        &home,
        AppSettings {
            hidden_agents: vec![
                AgentId::Muse,
                AgentId::Cursor,
                AgentId::Antigravity,
                AgentId::Codex,
            ],
            binary_paths: HashMap::from([
                (AgentId::Claude, "/opt/acme/bin/claude".to_string()),
                (AgentId::Codex, "/opt/acme/bin/codex".to_string()),
                (AgentId::Antigravity, "/opt/acme/bin/agy".to_string()),
                (AgentId::Cursor, "/opt/acme/bin/agent".to_string()),
                (AgentId::Muse, "/opt/acme/bin/muse".to_string()),
            ]),
            ..AppSettings::default()
        },
    )
    .unwrap();

    let saved: Value =
        serde_json::from_str(&fs::read_to_string(crate::paths::settings_path_for(&home)).unwrap())
            .unwrap();
    assert_eq!(
        saved["hiddenAgents"],
        serde_json::json!(["codex", "antigravity", "cursor", "muse"])
    );
    assert_eq!(
        saved["binaryPaths"],
        serde_json::json!({
            "claude": "/opt/acme/bin/claude",
            "codex": "/opt/acme/bin/codex",
            "antigravity": "/opt/acme/bin/agy",
            "cursor": "/opt/acme/bin/agent",
            "muse": "/opt/acme/bin/muse",
        })
    );
    let _ = fs::remove_dir_all(home);
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
    let err = refused_save(AppSettings {
        hidden_agents: AgentId::ALL.to_vec(),
        ..AppSettings::default()
    });
    assert!(
        err.message.contains("at least one provider"),
        "{}",
        err.message
    );
}

#[test]
fn any_one_provider_left_visible_is_enough_to_save() {
    let home = crate::paths::scratch_dir("settings-one-visible");
    for visible in AgentId::ALL {
        let hidden: Vec<AgentId> = AgentId::ALL
            .into_iter()
            .filter(|id| *id != visible)
            .collect();
        let saved = save_under(
            &home,
            AppSettings {
                hidden_agents: hidden.clone(),
                ..AppSettings::default()
            },
        )
        .unwrap();
        assert_eq!(saved.hidden_agents, hidden, "{visible:?} left visible");
    }
    let _ = fs::remove_dir_all(home);
}

#[test]
fn a_saved_document_is_the_validated_one_and_loads_back_unchanged() {
    let home = crate::paths::scratch_dir("settings-save");
    let settings = save_under(
        &home,
        AppSettings {
            hidden_agents: vec![AgentId::Cursor, AgentId::Claude, AgentId::Cursor],
            binary_paths: HashMap::from([
                (AgentId::Codex, "  ".to_string()),
                (AgentId::Claude, "/opt/acme/bin/claude".to_string()),
            ]),
            github_scopes: vec![
                "acme/webapp".into(),
                "org:acme".into(),
                "repo:acme/webapp".into(),
            ],
            limits_poll_minutes: 7,
            ..AppSettings::default()
        },
    )
    .unwrap();
    assert_eq!(
        settings.hidden_agents,
        vec![AgentId::Claude, AgentId::Cursor]
    );
    assert_eq!(
        settings.binary_paths,
        HashMap::from([(AgentId::Claude, "/opt/acme/bin/claude".to_string())])
    );
    assert_eq!(settings.github_scopes, vec!["repo:acme/webapp", "org:acme"]);
    assert_eq!(settings.limits_poll_minutes, 5);

    let saved = fs::read_to_string(crate::paths::settings_path_for(&home)).unwrap();
    assert_eq!(parse_settings(Some(&saved)), settings);
    let _ = fs::remove_dir_all(home);
}

#[test]
fn close_to_tray_defaults_off_and_survives_a_round_trip() {
    assert!(!parse_settings(Some(r#"{"limitsPollMinutes":10}"#)).close_to_tray);
    assert!(!AppSettings::default().close_to_tray);

    let enabled = parse_settings(Some(r#"{"closeToTray":true}"#));
    assert!(enabled.close_to_tray);

    let body = serde_json::to_string(&enabled).unwrap();
    assert!(parse_settings(Some(&body)).close_to_tray);
}

#[test]
fn settings_without_reset_alerts_load_with_none() {
    let settings = parse_settings(Some(r#"{"limitNotifications": true}"#));
    assert!(settings.reset_alerts.is_empty());
}

#[test]
fn reset_alerts_stay_within_the_spend_rule() {
    let settings = parse_settings(Some(
        r#"{"resetAlerts": {
            "acct-a": {"label": "a@example.com", "maxLeftPercent": 3, "minHoursToRenewal": 48},
            "acct-b": {"maxLeftPercent": 40, "minHoursToRenewal": 1000},
            "acct-c": {"maxLeftPercent": 0},
            "acct-d": {}
        }}"#,
    ));

    let alert = |id: &str| settings.reset_alerts.get(id).cloned().unwrap();
    assert_eq!(
        alert("acct-a"),
        ResetAlert {
            label: Some("a@example.com".into()),
            max_left_percent: 3,
            min_hours_to_renewal: 48,
            automatic: false,
        }
    );
    assert_eq!(alert("acct-b").max_left_percent, 5);
    assert_eq!(alert("acct-b").min_hours_to_renewal, 168);
    assert_eq!(alert("acct-c").max_left_percent, 1);
    assert_eq!(alert("acct-c").min_hours_to_renewal, 24);
    assert_eq!(
        alert("acct-d"),
        ResetAlert {
            label: None,
            max_left_percent: 5,
            min_hours_to_renewal: 24,
            automatic: false,
        }
    );
}

#[test]
fn one_setting_the_app_cannot_read_leaves_every_other_one() {
    let settings = parse_settings(Some(
        r#"{
            "githubScopes": ["org:acme"],
            "automaticUpdates": false,
            "closeToTray": true,
            "limitsPollMinutes": "ten",
            "githubPollSeconds": 120
        }"#,
    ));

    assert_eq!(settings.github_scopes, ["org:acme"]);
    assert!(!settings.automatic_updates);
    assert!(settings.close_to_tray);
    assert_eq!(settings.limits_poll_minutes, 5);
    assert_eq!(settings.github_poll_seconds, 120);
}

#[test]
fn an_out_of_range_alert_figure_keeps_the_alert_and_the_rest() {
    let settings = parse_settings(Some(
        r#"{
            "githubScopes": ["org:acme"],
            "resetAlerts": {
                "acct-a": {"label": "a@example.com", "maxLeftPercent": 260, "minHoursToRenewal": 100000},
                "acct-b": {"maxLeftPercent": 2.6, "minHoursToRenewal": -3, "automatic": "yes"}
            }
        }"#,
    ));

    assert_eq!(settings.github_scopes, ["org:acme"]);
    assert_eq!(
        settings.reset_alerts["acct-a"],
        ResetAlert {
            label: Some("a@example.com".into()),
            max_left_percent: 5,
            min_hours_to_renewal: 168,
            automatic: false,
        }
    );
    assert_eq!(settings.reset_alerts["acct-b"].max_left_percent, 3);
    assert!(!settings.reset_alerts["acct-b"].automatic);
    assert_eq!(settings.reset_alerts["acct-b"].min_hours_to_renewal, 0);
}

#[test]
fn an_unknown_provider_drops_only_its_own_entry() {
    let settings = parse_settings(Some(
        r#"{
            "hiddenAgents": ["codex", "gemini"],
            "binaryPaths": {"claude": "/opt/claude", "gemini": "/opt/gemini"},
            "githubScopes": ["org:acme", 42],
            "resetAlerts": {"acct-a": 5, "acct-b": {}}
        }"#,
    ));

    assert_eq!(settings.hidden_agents, [AgentId::Codex]);
    assert_eq!(settings.github_scopes, ["org:acme"]);
    assert_eq!(
        settings.binary_paths,
        HashMap::from([(AgentId::Claude, "/opt/claude".to_string())])
    );
    assert_eq!(
        settings.reset_alerts.keys().collect::<Vec<_>>(),
        ["acct-b"],
        "an alert that is not an object is dropped alone"
    );
}

#[test]
fn a_document_that_is_not_an_object_loads_as_defaults() {
    assert_eq!(parse_settings(Some("[1, 2]")), AppSettings::default());
    assert_eq!(parse_settings(Some("null")), AppSettings::default());
}

fn every_setting_away_from_its_default() -> AppSettings {
    AppSettings {
        hidden_agents: vec![AgentId::Antigravity],
        binary_paths: HashMap::from([(AgentId::Codex, "/opt/acme/bin/codex".to_string())]),
        automatic_updates: false,
        limit_notifications: true,
        limits_poll_minutes: 15,
        github_scopes: vec!["org:acme".into()],
        github_notifications: true,
        github_poll_seconds: 300,
        close_to_tray: true,
        reset_alerts: HashMap::from([(
            "acct-a".to_string(),
            ResetAlert {
                label: Some("you@example.com".into()),
                max_left_percent: 4,
                min_hours_to_renewal: 36,
                automatic: true,
            },
        )]),
    }
}

#[test]
fn the_documents_the_ui_checks_its_settings_against_are_the_ones_rust_writes() {
    for (name, fixture, settings) in [
        (
            "app-settings.json",
            include_str!("fixtures/app-settings.json"),
            every_setting_away_from_its_default(),
        ),
        (
            "app-settings.default.json",
            include_str!("fixtures/app-settings.default.json"),
            AppSettings::default(),
        ),
    ] {
        let written = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            written,
            serde_json::from_str::<Value>(fixture).unwrap(),
            "src-tauri/src/settings/fixtures/{name} must read:\n{}",
            serde_json::to_string_pretty(&written).unwrap()
        );
        assert_eq!(parse_settings(Some(fixture)), settings, "{name}");
    }
}

#[test]
fn every_setting_reads_back_as_it_was_written() {
    let settings = every_setting_away_from_its_default();
    let written = serde_json::to_value(&settings).unwrap();
    let defaults = serde_json::to_value(AppSettings::default()).unwrap();
    let alert_defaults = serde_json::to_value(ResetAlert::default()).unwrap();
    for (key, value) in written.as_object().unwrap() {
        assert_ne!(Some(value), defaults.get(key), "{key} is at its default");
    }
    for (key, value) in written["resetAlerts"]["acct-a"].as_object().unwrap() {
        assert_ne!(
            Some(value),
            alert_defaults.get(key),
            "resetAlerts.{key} is at its default"
        );
    }

    assert_eq!(parse_settings(Some(&written.to_string())), settings);
}

#[test]
fn a_file_starting_with_a_byte_order_mark_keeps_its_settings() {
    let settings = parse_settings(Some(
        "\u{feff}{\"closeToTray\": true, \"githubScopes\": [\"org:acme\"]}",
    ));

    assert!(settings.close_to_tray);
    assert_eq!(settings.github_scopes, ["org:acme"]);
}

#[test]
fn a_reset_alert_is_written_and_received_in_its_settings_shape() {
    let alert = ResetAlert {
        label: Some("you@example.com".into()),
        max_left_percent: 4,
        min_hours_to_renewal: 36,
        automatic: true,
    };

    assert_eq!(
        serde_json::to_value(&alert).unwrap(),
        serde_json::json!({
            "label": "you@example.com",
            "maxLeftPercent": 4,
            "minHoursToRenewal": 36,
            "automatic": true
        })
    );
    assert_eq!(
        serde_json::from_value::<ResetAlert>(serde_json::json!({})).unwrap(),
        ResetAlert {
            label: None,
            max_left_percent: 5,
            min_hours_to_renewal: 24,
            automatic: false,
        },
        "an alert received over IPC without its figures"
    );
}

#[test]
fn a_remembered_interval_reads_its_setting_once() {
    let memo = AtomicU64::new(0);
    let loads = std::cell::Cell::new(0);
    let load = || {
        loads.set(loads.get() + 1);
        120
    };
    assert_eq!(remembered_seconds(&memo, load), Duration::from_secs(120));
    assert_eq!(remembered_seconds(&memo, load), Duration::from_secs(120));
    assert_eq!(loads.get(), 1, "read once, then remembered");
}

#[test]
fn a_saved_interval_is_returned_without_reading_the_settings() {
    let memo = AtomicU64::new(30);
    assert_eq!(
        remembered_seconds(&memo, || panic!("the settings were read")),
        Duration::from_secs(30)
    );
}
