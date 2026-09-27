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
    assert!(user_home().is_err(), "a test build resolved a user home");
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

/// Outside a test build `ON_N_OFF_HOME` wins, then `USERPROFILE`, then `HOME`, and with none of
/// them there is no home.
#[test]
fn a_running_app_takes_its_home_from_on_n_off_home_then_userprofile_then_home() {
    type Environment = &'static [(&'static str, &'static str)];
    let cases: [(Environment, Option<&str>); 4] = [
        (
            &[
                ("ON_N_OFF_HOME", "/scratch/qa"),
                ("USERPROFILE", "/Users/me"),
                ("HOME", "/home/me"),
            ],
            Some("/scratch/qa"),
        ),
        (
            &[("USERPROFILE", "/Users/me"), ("HOME", "/home/me")],
            Some("/Users/me"),
        ),
        (&[("HOME", "/home/me")], Some("/home/me")),
        (&[], None),
    ];
    for (environment, expected) in cases {
        let home = user_home_from(|name| {
            environment
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        });
        assert_eq!(home.ok(), expected.map(PathBuf::from), "{environment:?}");
    }
}

/// Only this module resolves the user home. Reading the process's `HOME` or `USERPROFILE`, or
/// calling a `home_dir` function, anywhere else would reach a developer's real home from a test
/// build, where [`user_home`] has none.
#[test]
fn only_the_paths_module_reads_the_process_home() {
    // Where another file may name those variables, and why. An entry allows only the lines that
    // contain its text, so a new read in the same file still fails.
    const ALLOWED: [(&str, &str, &str); 3] = [
        (
            "accounts/claude.rs",
            "command.env(",
            "hands a Claude child a disposable OS home: sets, never reads",
        ),
        (
            "accounts/claude/tests/native_store.rs",
            "env.get(",
            "reads the environment a child command was handed",
        ),
        (
            "accounts/claude/tests/native_store.rs",
            "env.contains_key(",
            "reads the environment a child command was handed",
        ),
    ];
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    let mut pending = vec![src.clone()];
    while let Some(dir) = pending.pop() {
        for path in std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
        {
            let file = path
                .strip_prefix(&src)
                .unwrap()
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if file == "paths.rs" || file == "paths" {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (index, line) in text.lines().enumerate() {
                let names_the_home = ["\"HOME\"", "\"USERPROFILE\"", "home_dir("]
                    .iter()
                    .any(|needle| line.contains(needle));
                let allowed = ALLOWED
                    .iter()
                    .any(|(allowed, text, _why)| *allowed == file && line.contains(text));
                if names_the_home && !allowed {
                    found.push(format!("{file}:{}: {}", index + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "the process home is read outside paths.rs:\n{}",
        found.join("\n")
    );
}
