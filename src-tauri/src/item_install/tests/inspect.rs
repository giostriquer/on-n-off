use super::*;

#[test]
fn inspect_lists_plugins_skills_and_agents_from_plugin_json() {
    let h = Harness::new("items-inspect");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let dto = h
        .service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap();
    assert!(dto.is_marketplace);
    assert_eq!(dto.commit_sha, SHA_A);
    assert_eq!(dto.marketplace_name, "mattpocock");
    assert_eq!(dto.plugins.len(), 1);
    let plugin = &dto.plugins[0];
    assert_eq!(plugin.name, "mattpocock-skills");
    assert_eq!(plugin.version.as_deref(), Some("1.2.3"));
    assert!(plugin.supported);
    let skills: Vec<(&str, &str)> = plugin
        .skills
        .iter()
        .map(|s| (s.name.as_str(), s.path.as_str()))
        .collect();
    assert_eq!(
        skills,
        vec![
            ("tdd", "skills/engineering/tdd"),
            ("grilling", "skills/productivity/grilling")
        ]
    );
    assert_eq!(plugin.skills[0].description, "Test-driven development");
    assert_eq!(plugin.agents.len(), 1);
    assert_eq!(plugin.agents[0].name, "reviewer");
    assert_eq!(plugin.agents[0].path, "agents/reviewer.md");
    // A second inspect re-checks the ref's sha (one cheap request) but reuses the tarball.
    let calls = h.fetcher.calls().len();
    h.service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap();
    let new_calls = h.fetcher.calls()[calls..].to_vec();
    assert_eq!(
        new_calls,
        vec![fetch::commit_sha_url("mattpocock", "skills", "HEAD")]
    );
    h.finish();
}

#[test]
fn inspect_falls_back_to_scanning_skills_folder_and_flags_unsupported_sources() {
    let h = Harness::new("items-inspect-scan");
    let files = [
        (
            ".claude-plugin/marketplace.json",
            r#"{"name":"mkt","plugins":[{"name":"local","source":"./plugins/local"},{"name":"remote","source":{"source":"url","url":"https://example.com/x.git"}},{"name":"gh","source":{"source":"github","repo":"other/repo"}}]}"#,
        ),
        (
            "plugins/local/.claude-plugin/plugin.json",
            r#"{"name":"local"}"#,
        ),
        (
            "plugins/local/skills/alpha/SKILL.md",
            "---\nname: alpha\n---\n",
        ),
        ("plugins/local/skills/beta/SKILL.md", "no frontmatter"),
        ("plugins/local/skills/notes.md", "stray file"),
    ];
    let bytes = tarball(SHA_A, &files);
    h.route_repo("HEAD", SHA_A, bytes);
    h.fetcher.route(
        &fetch::tarball_url("other", "repo", "HEAD"),
        tarball_with_root(
            "repo-main",
            Some(SHA_B),
            &[("skills/gamma/SKILL.md", "---\nname: gamma\n---\n")],
        ),
    );
    let dto = h
        .service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap();
    assert_eq!(dto.plugins.len(), 3);
    let local = &dto.plugins[0];
    assert!(local.supported);
    let names: Vec<&str> = local.skills.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta"]);
    assert_eq!(local.skills[0].path, "plugins/local/skills/alpha");
    let remote = &dto.plugins[1];
    assert!(!remote.supported);
    assert!(remote.skills.is_empty());
    let gh = &dto.plugins[2];
    assert!(gh.supported);
    assert_eq!(gh.skills[0].name, "gamma");
    assert_eq!(gh.skills[0].path, "skills/gamma");
    assert_eq!(
        gh.source
            .as_ref()
            .map(|s| (s.owner.as_str(), s.repo.as_str())),
        Some(("other", "repo"))
    );
    h.finish();
}

#[test]
fn inspect_reports_non_marketplace_repos() {
    let h = Harness::new("items-inspect-none");
    h.route_repo("HEAD", SHA_A, tarball(SHA_A, &[("README.md", "hi")]));
    let dto = h
        .service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap();
    assert!(!dto.is_marketplace);
    assert!(dto.plugins.is_empty());
    assert!(dto.hint.is_some());
    h.finish();
}

#[test]
fn inspect_surfaces_download_failure() {
    let h = Harness::new("items-inspect-fail");
    h.fetcher
        .fail(&fetch::tarball_url("mattpocock", "skills", "HEAD"), "boom");
    let error = h
        .service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap_err();
    assert!(error.message.contains("boom"), "{error:?}");
    h.finish();
}
