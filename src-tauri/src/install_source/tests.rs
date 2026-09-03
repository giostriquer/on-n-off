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
    assert_eq!(
        parse_install_source("npx -y skills add vercel-labs/agent-skills -g --skill web-design")
            .unwrap(),
        InstallSource::NpxSkills {
            source: "vercel-labs/agent-skills".into(),
            skill: Some("web-design".into()),
        }
    );
    assert_eq!(
        parse_install_source("skills add anthropics/skills").unwrap(),
        InstallSource::NpxSkills {
            source: "anthropics/skills".into(),
            skill: None,
        }
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
        [
            "plugin",
            "install",
            "-s",
            "user",
            "-y",
            "workbench@workshop"
        ]
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
        [
            "plugin",
            "marketplace",
            "add",
            "--scope",
            "user",
            "acme/tools@main"
        ]
    );
    assert_eq!(
        git.codex_install_argv(),
        [
            "plugin",
            "marketplace",
            "add",
            "--json",
            "acme/tools",
            "--ref",
            "main"
        ]
    );
    assert_eq!(
        InstallSource::claude_uninstall_argv("workbench@workshop"),
        [
            "plugin",
            "uninstall",
            "-s",
            "user",
            "-y",
            "workbench@workshop"
        ]
    );
    assert_eq!(
        InstallSource::codex_uninstall_argv("workbench@workshop"),
        ["plugin", "remove", "--json", "workbench@workshop"]
    );
    assert_eq!(
        InstallSource::plugin_marketplace("workbench@workshop"),
        Some("workshop")
    );
    assert_eq!(InstallSource::plugin_marketplace("local-only"), None);
    assert_eq!(
        InstallSource::claude_marketplace_update_argv("workshop"),
        ["plugin", "marketplace", "update", "workshop"]
    );
    assert_eq!(
        InstallSource::claude_update_argv("workbench@workshop"),
        ["plugin", "update", "-s", "user", "-y", "workbench@workshop"]
    );
    assert_eq!(
        InstallSource::codex_marketplace_upgrade_argv("workshop"),
        ["plugin", "marketplace", "upgrade", "--json", "workshop"]
    );
    assert_eq!(
        InstallSource::codex_update_argv("workbench@workshop"),
        ["plugin", "add", "--json", "workbench@workshop"]
    );
    let npx = InstallSource::NpxSkills {
        source: "vercel-labs/agent-skills".into(),
        skill: Some("web-design".into()),
    };
    assert_eq!(
        npx.npx_skills_argv("claude-code"),
        [
            "-y",
            "skills",
            "add",
            "vercel-labs/agent-skills",
            "-g",
            "-y",
            "-a",
            "claude-code",
            "--skill",
            "web-design"
        ]
    );
    assert_eq!(npx.npx_skills_argv("codex")[7], "codex");
}
