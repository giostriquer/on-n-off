use super::*;

fn install_tdd(h: &Harness) -> String {
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    let result = h
        .install(req, vec![(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Installed);
    h.registry().items[0].id.clone()
}

#[test]
fn status_reports_current_modified_and_missing() {
    let h = Harness::new("items-status");
    install_tdd(&h);
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, false)
        .unwrap();
    assert_eq!(statuses.len(), 1);
    let s = &statuses[0];
    assert_eq!(s.name, "tdd");
    assert_eq!(s.display_name, "tdd");
    assert_eq!(s.installed_version.as_deref(), Some("1.2.3"));
    assert!(!s.modified);
    assert!(!s.missing);
    assert_eq!(s.upstream, ItemUpstream::Current);

    let dest = h.home.join(".claude/skills/tdd");
    fs::write(dest.join("SKILL.md"), "---\nname: my-tdd\n---\nedited").unwrap();
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, false)
        .unwrap();
    assert!(statuses[0].modified);
    assert_eq!(statuses[0].display_name, "my-tdd");

    fs::remove_dir_all(&dest).unwrap();
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, false)
        .unwrap();
    assert!(statuses[0].missing);
    // Other providers / scopes see nothing.
    assert!(h
        .service
        .item_update_status(AgentId::Codex, None, false)
        .unwrap()
        .is_empty());
    assert!(h
        .service
        .item_update_status(AgentId::Claude, Some("/some/project"), false)
        .unwrap()
        .is_empty());
    h.finish();
}

#[test]
fn status_detects_upstream_update_and_honours_dismiss() {
    let h = Harness::new("items-status-update");
    let id = install_tdd(&h);
    // Upstream advances with a changed skill.
    h.route_repo("HEAD", SHA_B, mattpocock_tarball(SHA_B, "1.3.0", "\nnew"));
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    assert_eq!(
        statuses[0].upstream,
        ItemUpstream::UpdateAvailable {
            commit_sha: SHA_B.into(),
            plugin_version: Some("1.3.0".into()),
        }
    );
    // Keep mine → dismissed for this sha only.
    let dismissed = h.service.update_item(&id, UpdateItemMode::Dismiss).unwrap();
    assert_eq!(dismissed.upstream, ItemUpstream::Current);
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    assert_eq!(statuses[0].upstream, ItemUpstream::Current);
    // Overwrite → files replaced, backup kept, registry moved to SHA_B.
    let updated = h
        .service
        .update_item(&id, UpdateItemMode::Overwrite)
        .unwrap();
    assert_eq!(updated.installed_sha, SHA_B);
    assert_eq!(updated.installed_version.as_deref(), Some("1.3.0"));
    assert!(read(&h.home.join(".claude/skills/tdd/SKILL.md")).ends_with("new"));
    assert!(h.home.join(".on-n-off/backups/claude/items").is_dir());
    h.finish();
}

#[test]
fn status_treats_untouched_item_as_current_when_only_other_files_changed() {
    let h = Harness::new("items-status-same");
    install_tdd(&h);
    // Upstream advanced but tdd is byte-identical.
    h.route_repo("HEAD", SHA_B, mattpocock_tarball(SHA_B, "1.3.0", ""));
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    assert_eq!(statuses[0].upstream, ItemUpstream::Current);
    assert_eq!(
        statuses[0].installed_sha, SHA_A,
        "status checks never write the registry"
    );
    assert_eq!(h.registry().items[0].installed.commit_sha, SHA_A);
    h.finish();
}

#[test]
fn status_is_unknown_when_upstream_unreachable() {
    let h = Harness::new("items-status-offline");
    install_tdd(&h);
    h.fetcher.fail(
        &fetch::commit_sha_url("mattpocock", "skills", "HEAD"),
        "offline",
    );
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    assert_eq!(statuses[0].upstream, ItemUpstream::Unknown);
    h.finish();
}

#[test]
fn status_uses_memoised_sha_unless_forced() {
    let h = Harness::new("items-status-memo");
    install_tdd(&h);
    let before = h.fetcher.calls().len();
    h.service
        .item_update_status(AgentId::Claude, None, false)
        .unwrap();
    assert_eq!(
        h.fetcher.calls().len(),
        before,
        "install already learned the sha"
    );
    h.service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    assert_eq!(h.fetcher.calls().len(), before + 1);
    h.finish();
}

#[test]
fn remove_item_deletes_folder_and_registry_row() {
    let h = Harness::new("items-remove");
    let id = install_tdd(&h);
    h.service.remove_item(&id).unwrap();
    assert!(!h.home.join(".claude/skills/tdd").exists());
    assert!(h.registry().items.is_empty());
    assert!(h.home.join(".on-n-off/backups/claude/items").is_dir());
    assert!(h.service.remove_item(&id).is_err());
    h.finish();
}

#[test]
fn status_lists_project_scoped_items_only_for_that_project() {
    let h = Harness::new("items-status-project");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let project = h.home.join("proj");
    fs::create_dir_all(&project).unwrap();
    let path = project.to_string_lossy().into_owned();
    let scope = ItemScope::Project {
        project_path: path.clone(),
    };
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    h.install(req, vec![(AgentId::Claude, scope)]).unwrap();
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, Some(&path), false)
        .unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].upstream, ItemUpstream::Current);
    // Trailing separator and (on Windows) a different case still name the same project.
    let with_sep = format!("{path}{}", std::path::MAIN_SEPARATOR);
    assert_eq!(
        h.service
            .item_update_status(AgentId::Claude, Some(&with_sep), false)
            .unwrap()
            .len(),
        1
    );
    if cfg!(windows) {
        assert_eq!(
            h.service
                .item_update_status(AgentId::Claude, Some(&path.to_uppercase()), false)
                .unwrap()
                .len(),
            1
        );
    }
    assert!(h
        .service
        .item_update_status(AgentId::Claude, None, false)
        .unwrap()
        .is_empty());
    let other = h.home.join("other").to_string_lossy().into_owned();
    assert!(h
        .service
        .item_update_status(AgentId::Claude, Some(&other), false)
        .unwrap()
        .is_empty());
    h.finish();
}

#[test]
fn dismiss_applies_to_one_upstream_sha_and_overwrite_clears_it() {
    let h = Harness::new("items-status-dismiss-scope");
    let id = install_tdd(&h);
    h.route_repo("HEAD", SHA_B, mattpocock_tarball(SHA_B, "1.3.0", "\nnew"));
    h.service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    h.service.update_item(&id, UpdateItemMode::Dismiss).unwrap();
    assert_eq!(h.registry().items[0].dismissed_sha.as_deref(), Some(SHA_B));
    // A further upstream change is offered again.
    h.route_repo("HEAD", SHA_C, mattpocock_tarball(SHA_C, "1.4.0", "\nnewer"));
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    assert_eq!(
        statuses[0].upstream,
        ItemUpstream::UpdateAvailable {
            commit_sha: SHA_C.into(),
            plugin_version: Some("1.4.0".into()),
        }
    );
    // Overwrite of a locally modified copy: backup holds the edit, registry moves on.
    fs::write(h.home.join(".claude/skills/tdd/SKILL.md"), "mine").unwrap();
    let before = h
        .service
        .item_update_status(AgentId::Claude, None, false)
        .unwrap();
    assert!(before[0].modified);
    let updated = h
        .service
        .update_item(&id, UpdateItemMode::Overwrite)
        .unwrap();
    assert!(!updated.modified);
    assert_eq!(updated.installed_sha, SHA_C);
    assert!(read(&h.home.join(".claude/skills/tdd/SKILL.md")).ends_with("newer"));
    let item = &h.registry().items[0];
    assert!(item.dismissed_sha.is_none());
    assert_eq!(item.installed.commit_sha, SHA_C);
    let backups: Vec<_> = fs::read_dir(h.home.join(".on-n-off/backups/claude/items"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(read(&backups[0].join("SKILL.md")), "mine");
    h.finish();
}

#[test]
fn agent_status_update_and_remove_round_trip() {
    let h = Harness::new("items-agent-lifecycle");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Agent, "agents/reviewer.md")],
        false,
    );
    let result = h
        .install(req, vec![(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Installed);
    let file = h.home.join(".claude/agents/reviewer.md");
    assert!(read(&file).contains("Be strict."));
    let item = h.registry().items[0].clone();
    assert_eq!(item.kind, ItemKind::Agent);
    assert_eq!(
        item.files.keys().cloned().collect::<Vec<_>>(),
        vec!["reviewer.md"]
    );
    let s = &h
        .service
        .item_update_status(AgentId::Claude, None, false)
        .unwrap()[0];
    assert!(!s.modified && !s.missing);
    assert_eq!(s.display_name, "reviewer");
    fs::write(&file, "---\nname: strict-reviewer\n---\nedited").unwrap();
    let s = &h
        .service
        .item_update_status(AgentId::Claude, None, false)
        .unwrap()[0];
    assert!(s.modified);
    assert_eq!(s.display_name, "strict-reviewer");
    // Upstream changes the agent body.
    let mut files = mattpocock_files("1.3.0", "");
    files.retain(|(p, _)| p != "agents/reviewer.md");
    files.push((
        "agents/reviewer.md".into(),
        "---\nname: reviewer\n---\nBe kind.\n".into(),
    ));
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    h.route_repo("HEAD", SHA_B, tarball(SHA_B, &borrowed));
    let s = &h
        .service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap()[0];
    assert!(matches!(s.upstream, ItemUpstream::UpdateAvailable { .. }));
    let updated = h
        .service
        .update_item(&item.id, UpdateItemMode::Overwrite)
        .unwrap();
    assert!(!updated.modified);
    assert!(read(&file).contains("Be kind."));
    let backups = h.home.join(".on-n-off/backups/claude/items");
    assert!(fs::read_dir(&backups)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("reviewer.md.")));
    fs::remove_file(&file).unwrap();
    assert!(
        h.service
            .item_update_status(AgentId::Claude, None, false)
            .unwrap()[0]
            .missing
    );
    h.service.remove_item(&item.id).unwrap();
    assert!(h.registry().items.is_empty());
    h.finish();
}

#[test]
fn status_is_unknown_when_tarball_download_fails_after_sha_moved() {
    let h = Harness::new("items-status-tarball-fail");
    install_tdd(&h);
    h.fetcher.route(
        &fetch::commit_sha_url("mattpocock", "skills", "HEAD"),
        SHA_B.as_bytes().to_vec(),
    );
    h.fetcher.fail(
        &fetch::tarball_url("mattpocock", "skills", "HEAD"),
        "offline",
    );
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    assert_eq!(statuses[0].upstream, ItemUpstream::Unknown);
    h.finish();
}

#[test]
fn a_malformed_registry_blocks_every_mutation_and_writes_nothing() {
    let h = Harness::new("items-registry-malformed");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let path = crate::paths::installed_items_path_for(&h.home);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{ not json").unwrap();
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    let error = h
        .install(req, vec![(AgentId::Claude, ItemScope::Global)])
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Parse);
    assert!(!h.home.join(".claude/skills").exists());
    assert!(h
        .service
        .item_update_status(AgentId::Claude, None, false)
        .is_err());
    assert!(h.service.update_item("x", UpdateItemMode::Dismiss).is_err());
    assert!(h.service.remove_item("x").is_err());
    assert_eq!(read(&path), "{ not json");
    h.finish();
}
