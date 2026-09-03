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
