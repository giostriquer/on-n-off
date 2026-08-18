use super::*;

#[test]
fn install_writes_skill_and_records_it() {
    let h = Harness::new("items-install");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let mut req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    req.targets = vec![crate::dto::ItemTarget {
        provider: AgentId::Claude,
        scope: ItemScope::Global,
    }];
    let targets = vec![(AgentId::Claude, ItemScope::Global)];
    let result = h.install(req, targets).unwrap();
    assert_eq!(result.commit_sha, SHA_A);
    assert!(!result.sha_moved);
    assert_eq!(result.outcomes.len(), 1);
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Installed);
    let dest = h.home.join(".claude/skills/tdd");
    assert!(read(&dest.join("SKILL.md")).contains("name: tdd"));
    assert_eq!(read(&dest.join("reference.md")), "red, green, refactor");
    let reg = h.registry();
    assert_eq!(reg.items.len(), 1);
    let item = &reg.items[0];
    assert_eq!(item.installed.commit_sha, SHA_A);
    assert_eq!(item.installed.plugin_version.as_deref(), Some("1.2.3"));
    assert_eq!(item.source.upstream_path, "skills/engineering/tdd");
    assert_eq!(item.files.len(), 3);
    assert!(item.files.contains_key("SKILL.md"));
    assert!(
        item.files.contains_key("ref/notes.md"),
        "nested files use / keys"
    );
    assert_eq!(read(&dest.join("ref").join("notes.md")), "nested");
    // Claude sees it as a user skill.
    let tab = h.claude().list_tab().unwrap();
    assert!(tab.user_skills.iter().any(|s| s.name == "tdd"));
    h.finish();
}

#[test]
fn install_into_several_providers_and_skips_agents_outside_claude() {
    let h = Harness::new("items-install-multi");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let req = request(
        SHA_A,
        vec![
            pick(ItemKind::Skill, "skills/engineering/tdd"),
            pick(ItemKind::Agent, "agents/reviewer.md"),
        ],
        false,
    );
    let targets = vec![
        (AgentId::Claude, ItemScope::Global),
        (AgentId::Codex, ItemScope::Global),
        (AgentId::Antigravity, ItemScope::Global),
        (AgentId::Cursor, ItemScope::Global),
    ];
    let result = h.install(req, targets).unwrap();
    let statuses: Vec<(AgentId, ItemKind, ItemOutcomeStatus)> = result
        .outcomes
        .iter()
        .map(|o| (o.provider, o.kind, o.status))
        .collect();
    assert_eq!(
        statuses,
        vec![
            (
                AgentId::Claude,
                ItemKind::Skill,
                ItemOutcomeStatus::Installed
            ),
            (
                AgentId::Claude,
                ItemKind::Agent,
                ItemOutcomeStatus::Installed
            ),
            (
                AgentId::Codex,
                ItemKind::Skill,
                ItemOutcomeStatus::Installed
            ),
            (AgentId::Codex, ItemKind::Agent, ItemOutcomeStatus::Skipped),
            (
                AgentId::Antigravity,
                ItemKind::Skill,
                ItemOutcomeStatus::Installed
            ),
            (
                AgentId::Antigravity,
                ItemKind::Agent,
                ItemOutcomeStatus::Skipped
            ),
            (
                AgentId::Cursor,
                ItemKind::Skill,
                ItemOutcomeStatus::Installed
            ),
            (AgentId::Cursor, ItemKind::Agent, ItemOutcomeStatus::Skipped),
        ]
    );
    assert!(h.home.join(".claude/agents/reviewer.md").is_file());
    assert!(h.home.join(".codex/skills/tdd/SKILL.md").is_file());
    assert!(h
        .home
        .join(".gemini/antigravity-cli/skills/tdd/SKILL.md")
        .is_file());
    assert!(h.home.join(".cursor/skills/tdd/SKILL.md").is_file());
    let anti = AntigravityAdapter::at(h.home.join(".gemini"))
        .list_tab()
        .unwrap();
    assert!(anti.user_skills.iter().any(|s| s.name == "tdd"));
    assert_eq!(h.registry().items.len(), 5);
    // Only one tarball download for the whole batch.
    let tarballs = h
        .fetcher
        .calls()
        .iter()
        .filter(|u| u.contains("codeload"))
        .count();
    assert_eq!(tarballs, 1);
    h.finish();
}

#[test]
fn install_into_project_scope_creates_provider_dirs() {
    let h = Harness::new("items-install-project");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let project = h.home.join("proj");
    fs::create_dir_all(&project).unwrap();
    let scope = ItemScope::Project {
        project_path: project.to_string_lossy().into_owned(),
    };
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    let targets = vec![
        (AgentId::Claude, scope.clone()),
        (AgentId::Codex, scope.clone()),
    ];
    let result = h.install(req, targets).unwrap();
    assert!(result
        .outcomes
        .iter()
        .all(|o| o.status == ItemOutcomeStatus::Installed));
    assert!(project.join(".claude/skills/tdd/SKILL.md").is_file());
    assert!(project.join(".codex/skills/tdd/SKILL.md").is_file());
    assert!(h.registry().items.iter().all(|i| i.scope == scope));

    // A project folder that does not exist fails without creating anything.
    let missing = ItemScope::Project {
        project_path: h.home.join("nope").to_string_lossy().into_owned(),
    };
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    let result = h.install(req, vec![(AgentId::Claude, missing)]).unwrap();
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Failed);
    assert!(!h.home.join("nope").exists());
    h.finish();
}

#[test]
fn install_conflicts_with_unmanaged_folder_unless_overwrite() {
    let h = Harness::new("items-install-conflict");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let dest = h.home.join(".claude/skills/tdd");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("SKILL.md"), "mine").unwrap();
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    let result = h
        .install(req, vec![(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Conflict);
    assert_eq!(read(&dest.join("SKILL.md")), "mine");
    assert!(h.registry().items.is_empty());

    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        true,
    );
    let result = h
        .install(req, vec![(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Replaced);
    assert!(read(&dest.join("SKILL.md")).contains("name: tdd"));
    let backups = h.home.join(".on-n-off/backups/claude/items");
    assert!(backups.is_dir(), "backup of the unmanaged folder is kept");
    assert_eq!(h.registry().items.len(), 1);
    h.finish();
}

#[test]
fn install_replaces_managed_item_with_backup_and_dedupes_batch() {
    let h = Harness::new("items-install-replace");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    h.install(req, vec![(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    // Same skill twice in one batch (e.g. two plugins shipping `tdd`): second is a conflict.
    let mut second = pick(ItemKind::Skill, "skills/engineering/tdd");
    second.plugin_name = "other-plugin".into();
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd"), second],
        false,
    );
    let result = h
        .install(req, vec![(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Replaced);
    assert_eq!(result.outcomes[1].status, ItemOutcomeStatus::Skipped);
    assert!(result.outcomes[1]
        .reason
        .as_deref()
        .is_some_and(|r| r.contains("also selected")));
    assert_eq!(result.outcomes[1].plugin_name, "other-plugin");
    assert_eq!(result.outcomes[1].path, "skills/engineering/tdd");
    assert_eq!(h.registry().items.len(), 1);
    let backups = fs::read_dir(h.home.join(".on-n-off/backups/claude/items"))
        .unwrap()
        .count();
    assert_eq!(backups, 1);
    h.finish();
}

#[test]
fn install_refetches_when_sha_moved_and_reports_it() {
    let h = Harness::new("items-install-moved");
    h.route_repo("HEAD", SHA_B, mattpocock_tarball(SHA_B, "1.3.0", "\nnew"));
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    let result = h
        .install(req, vec![(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert!(result.sha_moved);
    assert_eq!(result.commit_sha, SHA_B);
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Installed);
    assert_eq!(h.registry().items[0].installed.commit_sha, SHA_B);
    h.finish();
}

#[test]
fn install_with_no_items_does_nothing() {
    let h = Harness::new("items-install-empty");
    let req = request(SHA_A, Vec::new(), false);
    let result = h
        .install(req, vec![(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert!(result.outcomes.is_empty());
    assert!(h.fetcher.calls().is_empty());
    assert!(h.registry().items.is_empty());
    h.finish();
}

#[test]
fn install_reports_missing_upstream_path_as_failed() {
    let h = Harness::new("items-install-missing-path");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/gone")],
        false,
    );
    let result = h
        .install(req, vec![(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Failed);
    assert!(h.registry().items.is_empty());
    h.finish();
}

#[test]
fn install_reports_write_failure_and_leaves_registry_untouched() {
    let h = Harness::new("items-install-write-fail");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    // A regular file where the skills folder should be makes every Claude write fail.
    fs::create_dir_all(h.home.join(".claude")).unwrap();
    fs::write(h.home.join(".claude/skills"), "not a folder").unwrap();
    let req = request(
        SHA_A,
        vec![
            pick(ItemKind::Skill, "skills/engineering/tdd"),
            pick(ItemKind::Skill, "skills/productivity/grilling"),
        ],
        false,
    );
    let result = h
        .install(
            req,
            vec![
                (AgentId::Claude, ItemScope::Global),
                (AgentId::Codex, ItemScope::Global),
            ],
        )
        .unwrap();
    let claude: Vec<_> = result
        .outcomes
        .iter()
        .filter(|o| o.provider == AgentId::Claude)
        .collect();
    assert_eq!(claude.len(), 2);
    assert!(claude
        .iter()
        .all(|o| o.status == ItemOutcomeStatus::Failed && o.reason.is_some()));
    // The batch continues for the other provider.
    assert!(result
        .outcomes
        .iter()
        .filter(|o| o.provider == AgentId::Codex)
        .all(|o| o.status == ItemOutcomeStatus::Installed));
    assert_eq!(read(&h.home.join(".claude/skills")), "not a folder");
    assert!(h
        .registry()
        .items
        .iter()
        .all(|i| i.provider == AgentId::Codex));
    assert!(!fs::read_dir(h.home.join(".claude"))
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().contains("tmp-on-n-off")));
    h.finish();
}

#[test]
fn install_uses_the_snapshot_the_user_saw_when_upstream_moves_after_inspect() {
    let h = Harness::new("items-install-memo");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let seen = h
        .service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap();
    // Upstream advances between the sheet opening and Install being clicked.
    h.route_repo("HEAD", SHA_B, mattpocock_tarball(SHA_B, "1.3.0", "\nnew"));
    let downloads = |h: &Harness| {
        h.fetcher
            .calls()
            .iter()
            .filter(|u| u.contains("codeload"))
            .count()
    };
    let before = downloads(&h);
    let req = request(
        &seen.commit_sha,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    let result = h
        .install(req, vec![(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert!(!result.sha_moved);
    assert_eq!(result.commit_sha, SHA_A);
    assert_eq!(h.registry().items[0].installed.commit_sha, SHA_A);
    assert!(!read(&h.home.join(".claude/skills/tdd/SKILL.md")).ends_with("new"));
    assert_eq!(downloads(&h), before, "the memoised snapshot is reused");
    h.finish();
}

#[test]
fn install_and_status_follow_the_requested_ref() {
    let h = Harness::new("items-ref");
    h.route_repo("v1", SHA_A, mattpocock_tarball(SHA_A, "1.0.0", ""));
    h.route_repo("HEAD", SHA_B, mattpocock_tarball(SHA_B, "2.0.0", "\nnew"));
    let dto = h
        .service
        .inspect_marketplace("mattpocock", "skills", Some("v1"))
        .unwrap();
    assert_eq!(dto.commit_sha, SHA_A);
    assert_eq!(dto.plugins[0].version.as_deref(), Some("1.0.0"));
    let mut req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    req.source.git_ref = "v1".into();
    let result = h
        .install(req, vec![(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert_eq!(result.commit_sha, SHA_A);
    let item = &h.registry().items[0];
    assert_eq!(item.source.git_ref, "v1");
    assert_eq!(item.installed.plugin_version.as_deref(), Some("1.0.0"));
    let before = h.fetcher.calls().len();
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    assert_eq!(statuses[0].upstream, ItemUpstream::Current);
    let asked: Vec<String> = h.fetcher.calls()[before..].to_vec();
    assert_eq!(
        asked,
        vec![fetch::commit_sha_url("mattpocock", "skills", "v1")]
    );
    h.finish();
}
