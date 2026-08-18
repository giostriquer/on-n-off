use super::*;

/// Real network: inspects mattpocock/skills, installs two skills into a scratch home, checks
/// status. Run explicitly with `cargo test real_github -- --ignored`.
#[test]
#[ignore = "talks to GitHub"]
fn real_github_marketplace_round_trip() {
    let home = scratch_dir("items-real-github");
    let service = ItemService::at(home.clone(), Box::new(fetch::HttpFetcher));
    let dto = service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap();
    assert!(dto.is_marketplace, "{dto:?}");
    assert!(fetch::is_full_sha(&dto.commit_sha));
    let plugin = &dto.plugins[0];
    assert!(plugin.supported);
    assert!(plugin.skills.len() >= 10, "{}", plugin.skills.len());
    let tdd = plugin
        .skills
        .iter()
        .find(|s| s.name == "tdd")
        .expect("tdd skill listed");
    let picks = vec![
        ItemPick {
            plugin_name: plugin.name.clone(),
            kind: ItemKind::Skill,
            path: tdd.path.clone(),
            source: None,
        },
        ItemPick {
            plugin_name: plugin.name.clone(),
            kind: ItemKind::Skill,
            path: plugin.skills[1].path.clone(),
            source: None,
        },
    ];
    let req = InstallItemsRequest {
        source: source(),
        commit_sha: dto.commit_sha.clone(),
        items: picks,
        targets: Vec::new(),
        overwrite_unmanaged: false,
    };
    let claude = ClaudeAdapter::at(home.join(".claude"));
    let codex = CodexAdapter::at(home.join(".codex"), home.join(".agents/skills"));
    let resolve = |target: &ItemTarget| match target.provider {
        AgentId::Claude => claude.item_roots(&target.scope),
        _ => codex.item_roots(&target.scope),
    };
    let mut req = req;
    req.targets = vec![
        ItemTarget {
            provider: AgentId::Claude,
            scope: ItemScope::Global,
        },
        ItemTarget {
            provider: AgentId::Codex,
            scope: ItemScope::Global,
        },
    ];
    let result = service.install_items(req, &resolve).unwrap();
    eprintln!("{result:#?}");
    assert!(result
        .outcomes
        .iter()
        .all(|o| o.status == ItemOutcomeStatus::Installed));
    assert!(home.join(".claude/skills/tdd/SKILL.md").is_file());
    assert!(home.join(".codex/skills/tdd/SKILL.md").is_file());
    assert!(claude
        .list_tab()
        .unwrap()
        .user_skills
        .iter()
        .any(|s| s.name == "tdd"));
    let statuses = service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    eprintln!("{statuses:#?}");
    assert_eq!(statuses.len(), 2);
    assert!(statuses.iter().all(|s| s.upstream == ItemUpstream::Current));
    assert!(statuses.iter().all(|s| s.installed_version.is_some()));
    let _ = fs::remove_dir_all(home);
}
