use super::*;
use crate::dto::{DepConfidence, ItemDependencyDto};
use crate::item_install::deps::{self, EntryRef, SiblingIndex};

// Names here are invented ("acme" plugin, `deploy`, `lint`, …); they encode nothing about any
// real marketplace, so a change upstream cannot silently make these tests describe the wrong
// behaviour.

fn entry(plugin: &str, kind: ItemKind, path: &str, name: &str) -> EntryRef {
    EntryRef {
        plugin_name: plugin.into(),
        kind,
        path: path.into(),
        name: name.into(),
    }
}

fn skill(plugin: &str, name: &str) -> EntryRef {
    entry(plugin, ItemKind::Skill, &format!("skills/{name}"), name)
}

fn files(text: &str) -> write::ItemFiles {
    let mut files = write::ItemFiles::new();
    files.insert("SKILL.md".to_string(), text.as_bytes().to_vec());
    files
}

fn dep(plugin: &str, kind: ItemKind, name: &str, confidence: DepConfidence) -> ItemDependencyDto {
    let path = match kind {
        ItemKind::Skill => format!("skills/{name}"),
        ItemKind::Agent => format!("agents/{name}.md"),
    };
    ItemDependencyDto {
        plugin_name: plugin.into(),
        kind,
        path,
        name: name.into(),
        confidence,
    }
}

/// A "router" skill whose prose sends the user to its siblings in every form we grade.
#[test]
fn detect_ranks_mentions_and_skips_self_and_short_names() {
    let me = skill("acme", "router");
    let index = SiblingIndex::from_entries(vec![
        me.clone(),
        skill("acme", "plan-with-docs"),
        skill("acme", "to-spec"),
        skill("acme", "to-tasks"),
        skill("acme", "build"),
        skill("acme", "verify"),
        skill("acme", "sort"),
        skill("acme", "probe"),
        skill("acme", "guide"),
        skill("acme", "go"),
        skill("acme", "docs"),
        entry("acme", ItemKind::Agent, "agents/auditor.md", "auditor"),
    ]);
    let text = "\
Route the user:
- questions -> /plan-with-docs (docs first, then decide)
- specs -> `/to-spec` and later `/to-tasks`.
- building -> run `/build`, which drives Skill(verify) and `--skill sort`
- pushback -> call the Skill tool twice, for \"probe\" and \"docs\"
- the guide skill is for guided setup
- ask `auditor` for a second opinion
- /router must never call itself; /go is too short to count
";
    let found = deps::detect(&files(text), &me, &index);
    assert_eq!(
        found.depends_on,
        vec![
            dep("acme", ItemKind::Skill, "build", DepConfidence::High),
            dep("acme", ItemKind::Skill, "docs", DepConfidence::High),
            dep("acme", ItemKind::Skill, "guide", DepConfidence::Medium),
            dep(
                "acme",
                ItemKind::Skill,
                "plan-with-docs",
                DepConfidence::High
            ),
            dep("acme", ItemKind::Skill, "probe", DepConfidence::High),
            dep("acme", ItemKind::Skill, "sort", DepConfidence::High),
            dep("acme", ItemKind::Skill, "to-spec", DepConfidence::High),
            dep("acme", ItemKind::Skill, "to-tasks", DepConfidence::High),
            dep("acme", ItemKind::Skill, "verify", DepConfidence::High),
            dep("acme", ItemKind::Agent, "auditor", DepConfidence::High),
        ]
    );
    assert!(found.external_refs.is_empty());
    assert!(!found.uses_plugin_root);
}

#[test]
fn detect_matches_whole_names_and_the_highest_confidence_wins() {
    let me = skill("acme", "build");
    let index = SiblingIndex::from_entries(vec![
        me.clone(),
        skill("acme", "verify"),
        skill("acme", "plan"),
        skill("acme", "plan-with-docs"),
    ]);
    // `the verify` (medium) and `/verify` (high) -> one high edge; `/plan-with-docs` must not
    // count as a mention of `plan`; a slash inside a URL is not a slash command.
    let text =
        "Use the verify loop; start with /verify. Then /plan-with-docs. See https://x.y/plan";
    let found = deps::detect(&files(text), &me, &index);
    assert_eq!(
        found.depends_on,
        vec![
            dep(
                "acme",
                ItemKind::Skill,
                "plan-with-docs",
                DepConfidence::High
            ),
            dep("acme", ItemKind::Skill, "verify", DepConfidence::High),
        ]
    );
}

#[test]
fn detect_prefers_a_same_plugin_sibling_over_a_same_named_skill_elsewhere() {
    let index = SiblingIndex::from_entries(vec![
        skill("a", "build"),
        skill("a", "verify"),
        skill("b", "verify"),
        skill("c", "wrap"),
    ]);
    let text = files("Run `/verify` first.");
    let from_a = deps::detect(&text, &skill("a", "build"), &index);
    assert_eq!(
        from_a.depends_on,
        vec![dep("a", ItemKind::Skill, "verify", DepConfidence::High)]
    );
    // No same-plugin candidate: the first plugin in marketplace order wins, once.
    let from_c = deps::detect(&text, &skill("c", "wrap"), &index);
    assert_eq!(
        from_c.depends_on,
        vec![dep("a", ItemKind::Skill, "verify", DepConfidence::High)]
    );
}

#[test]
fn detect_reads_every_text_file_and_matches_folder_names_too() {
    // Frontmatter says "Plan With Docs" but the folder is `plan-with-docs`; prose uses the
    // folder name. Nested reference files count; binary files are ignored.
    let me = skill("acme", "router");
    let index = SiblingIndex::from_entries(vec![
        me.clone(),
        entry(
            "acme",
            ItemKind::Skill,
            "skills/plan-with-docs",
            "Plan With Docs",
        ),
        skill("acme", "verify"),
    ]);
    let mut all = files("Nothing here.");
    all.insert(
        "reference/flow.md".to_string(),
        b"Start with /plan-with-docs.".to_vec(),
    );
    all.insert(
        "bin/blob".to_string(),
        vec![0xff, 0xfe, b'/', b'v', b'e', b'r', b'i', b'f', b'y'],
    );
    let found = deps::detect(&all, &me, &index);
    assert_eq!(
        found.depends_on,
        vec![ItemDependencyDto {
            plugin_name: "acme".into(),
            kind: ItemKind::Skill,
            path: "skills/plan-with-docs".into(),
            name: "Plan With Docs".into(),
            confidence: DepConfidence::High,
        }]
    );
}

#[test]
fn detect_flags_plugin_root_and_paths_outside_the_item() {
    let me = entry("acme", ItemKind::Skill, "skills/ops/build", "build");
    let index = SiblingIndex::from_entries(vec![
        me.clone(),
        entry("acme", ItemKind::Skill, "skills/ops/verify", "verify"),
    ]);
    let text = "\
Read ../lib/impl.md and ${CLAUDE_PLUGIN_ROOT}/scripts/run.sh.
Templates live in skills/shared/templates/spec.md; our own notes in skills/ops/build/notes.md.
The loop is described in skills/ops/verify/SKILL.md (also reachable as ../verify/SKILL.md).
";
    let found = deps::detect(&files(text), &me, &index);
    assert!(found.uses_plugin_root);
    assert_eq!(
        found.external_refs,
        vec![
            "../lib/impl.md".to_string(),
            "skills/shared/templates/spec.md".to_string()
        ]
    );
    // Paths into a sibling are dependencies, not foreign assets.
    assert_eq!(
        found.depends_on,
        vec![ItemDependencyDto {
            plugin_name: "acme".into(),
            kind: ItemKind::Skill,
            path: "skills/ops/verify".into(),
            name: "verify".into(),
            confidence: DepConfidence::High,
        }]
    );
}

/// One plugin at the repository root: `deploy` names `rollback` and the `auditor` agent.
fn acme_files(deploy_body: &str) -> Vec<(String, String)> {
    vec![
        (
            ".claude-plugin/marketplace.json".to_string(),
            r#"{"name":"acme","plugins":[{"name":"acme-skills","source":"./"}]}"#.to_string(),
        ),
        (
            ".claude-plugin/plugin.json".to_string(),
            r#"{"name":"acme-skills","version":"0.1.0","skills":["./skills/ops/deploy","./skills/ops/rollback"]}"#.to_string(),
        ),
        (
            "skills/ops/deploy/SKILL.md".to_string(),
            format!("{}{deploy_body}", skill_md("deploy", "Ship a release")),
        ),
        (
            "skills/ops/rollback/SKILL.md".to_string(),
            skill_md("rollback", "Undo a release"),
        ),
        (
            "agents/auditor.md".to_string(),
            "---\nname: auditor\ndescription: Checks the diff\n---\nBe strict.\n".to_string(),
        ),
    ]
}

fn acme_tarball(sha: &str, deploy_body: &str) -> Vec<u8> {
    let files = acme_files(deploy_body);
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    tarball(sha, &borrowed)
}

#[test]
fn inspect_reports_dependencies_between_entries() {
    let h = Harness::new("items-deps-inspect");
    h.route_repo(
        "HEAD",
        SHA_A,
        acme_tarball(
            SHA_A,
            "\nIf it fails, run `/rollback`; hand the diff to `auditor`. Consider the deploy skill.\n",
        ),
    );
    let dto = h
        .service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap();
    let plugin = &dto.plugins[0];
    assert!(plugin.extras.is_empty(), "{:?}", plugin.extras);
    let deploy = &plugin.skills[0];
    assert_eq!(deploy.name, "deploy");
    assert_eq!(
        deploy.depends_on,
        vec![
            ItemDependencyDto {
                plugin_name: "acme-skills".into(),
                kind: ItemKind::Skill,
                path: "skills/ops/rollback".into(),
                name: "rollback".into(),
                confidence: DepConfidence::High,
            },
            ItemDependencyDto {
                plugin_name: "acme-skills".into(),
                kind: ItemKind::Agent,
                path: "agents/auditor.md".into(),
                name: "auditor".into(),
                confidence: DepConfidence::High,
            },
        ]
    );
    assert!(deploy.external_refs.is_empty());
    assert!(!deploy.uses_plugin_root);
    assert!(plugin.skills[1].depends_on.is_empty());
    assert!(plugin.agents[0].depends_on.is_empty());
    h.finish();
}

#[test]
fn inspect_lists_plugin_extras_a_local_copy_never_gets() {
    let h = Harness::new("items-deps-extras");
    let files = [
        (
            ".claude-plugin/marketplace.json",
            r#"{"name":"mkt","plugins":[{"name":"full","source":"./plugins/full"},{"name":"hooked","source":"./plugins/hooked"},{"name":"bare","source":"./plugins/bare"}]}"#,
        ),
        (
            "plugins/full/.claude-plugin/plugin.json",
            r#"{"name":"full"}"#,
        ),
        (
            "plugins/full/skills/alpha/SKILL.md",
            "---\nname: alpha\n---\n",
        ),
        ("plugins/full/commands/deploy.md", "# deploy"),
        ("plugins/full/.mcp.json", r#"{"mcpServers":{}}"#),
        (
            "plugins/hooked/.claude-plugin/plugin.json",
            r#"{"name":"hooked","mcpServers":{"x":{"command":"x"}}}"#,
        ),
        (
            "plugins/hooked/skills/beta/SKILL.md",
            "---\nname: beta\n---\n",
        ),
        ("plugins/hooked/hooks/hooks.json", "{}"),
        (
            "plugins/bare/.claude-plugin/plugin.json",
            r#"{"name":"bare"}"#,
        ),
        (
            "plugins/bare/skills/gamma/SKILL.md",
            "---\nname: gamma\n---\n",
        ),
    ];
    h.route_repo("HEAD", SHA_A, tarball(SHA_A, &files));
    let dto = h
        .service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap();
    let extras: Vec<Vec<String>> = dto.plugins.iter().map(|p| p.extras.clone()).collect();
    assert_eq!(
        extras,
        vec![
            vec!["commands".to_string(), "mcp".to_string()],
            vec!["hooks".to_string(), "mcp".to_string()],
            Vec::<String>::new(),
        ]
    );
    h.finish();
}

#[test]
fn install_records_high_confidence_dependencies_in_the_registry() {
    let h = Harness::new("items-deps-registry");
    h.route_repo(
        "HEAD",
        SHA_A,
        acme_tarball(
            SHA_A,
            "\nRun `/rollback` if needed and consider the auditor agent.\n",
        ),
    );
    h.service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap();
    h.install(
        request(
            SHA_A,
            vec![ItemPick {
                plugin_name: "acme-skills".into(),
                kind: ItemKind::Skill,
                path: "skills/ops/deploy".into(),
                source: None,
            }],
            false,
        ),
        vec![(AgentId::Claude, ItemScope::Global)],
    )
    .unwrap();
    let registry = h.registry();
    assert_eq!(registry.items.len(), 1);
    // `the auditor` is only a medium mention: not recorded.
    assert_eq!(
        registry.items[0].source.depends_on,
        vec!["acme-skills/skill/skills/ops/rollback".to_string()]
    );
    h.finish();
}

#[test]
fn registry_reads_files_written_before_dependencies_existed() {
    let home = scratch_dir("items-deps-registry-compat");
    let path = crate::paths::installed_items_path_for(&home);
    let mut file = InstalledItemsFile::default();
    let mut item = sample_item(&home, "deploy");
    item.source.depends_on = vec!["acme-skills/skill/skills/ops/rollback".into()];
    file.upsert(item.clone());
    registry::save(&path, &file).unwrap();
    let text = read(&path);
    assert!(text.contains("\"dependsOn\""), "{text}");
    assert_eq!(registry::load(&path).unwrap(), file);

    let mut legacy: serde_json::Value = serde_json::from_str(&text).unwrap();
    legacy["items"][0]["source"]
        .as_object_mut()
        .unwrap()
        .remove("dependsOn");
    let legacy = serde_json::to_string_pretty(&legacy).unwrap();
    assert!(!legacy.contains("dependsOn"), "{legacy}");
    fs::write(&path, legacy).unwrap();
    let loaded = registry::load(&path).unwrap();
    assert!(loaded.items[0].source.depends_on.is_empty());
    let _ = fs::remove_dir_all(home);
}
