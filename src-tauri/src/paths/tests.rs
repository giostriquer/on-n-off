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

/// Every agent home and on-n-off's own data sit under the user home, so a test build that
/// resolved one would let a test read, or write, a developer's real `~/.claude`, `~/.codex` or
/// `~/.on-n-off`. It must resolve none, whatever the process environment says.
#[test]
fn a_test_build_resolves_no_user_home() {
    let process_home = ["ON_N_OFF_HOME", "USERPROFILE", "HOME"]
        .into_iter()
        .find_map(std::env::var_os);
    if let Ok(home) = user_home() {
        panic!(
            "a test build resolved a user home (the process's own: {})",
            Some(home.as_os_str()) == process_home.as_deref()
        );
    }
    type Helper = fn() -> Result<PathBuf, AdapterError>;
    let helpers: [(&str, Helper); 16] = [
        ("claude_root", claude_root),
        ("codex_root", codex_root),
        ("agents_skills_root", agents_skills_root),
        ("gemini_root", gemini_root),
        ("cursor_root", cursor_root),
        ("antigravity_cli_root", antigravity_cli_root),
        ("antigravity_config_plugins", antigravity_config_plugins),
        ("antigravity_cli_plugins", antigravity_cli_plugins),
        ("antigravity_mcp_config", antigravity_mcp_config),
        ("antigravity_cli_skills", antigravity_cli_skills),
        ("backup_root", backup_root),
        ("flags_path", flags_path),
        ("settings_path", settings_path),
        ("limits_monitor_state_path", limits_monitor_state_path),
        ("github_monitor_state_path", github_monitor_state_path),
        ("installed_items_path", installed_items_path),
    ];
    for (name, helper) in helpers {
        assert!(helper().is_err(), "{name} resolved a path in a test build");
    }
}
