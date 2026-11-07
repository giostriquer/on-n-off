use super::*;
use crate::adapter::scratch_adapter;
use crate::dto::AgentId;

mod views;

#[test]
fn normalizes_windows_project_keys() {
    assert_eq!(
        normalize_project_key(r"E:\dev\on-n-off\"),
        if cfg!(windows) {
            "e:/dev/on-n-off"
        } else {
            "E:/dev/on-n-off"
        }
    );
    assert_eq!(project_label(r"E:\dev\on-n-off"), "on-n-off");
    assert_eq!(project_label(r"E:\dev\"), "dev");
}

#[test]
fn parses_claude_and_codex_recognized_projects() {
    let claude = parse_claude_projects(
        r#"{ "projects": { "E:/dev/on-n-off": {}, "C:/tmp/app": { "mcpServers": {} } } }"#,
    );
    assert_eq!(claude.len(), 2);
    let codex = parse_codex_projects(
        "[projects.'E:\\dev\\on-n-off']\ntrust_level = \"trusted\"\n\n[projects.'E:\\dev\\acme']\ntrust_level = \"trusted\"\n",
    );
    assert!(codex.iter().any(|path| path.contains("on-n-off")));
    assert!(codex.iter().any(|path| path.contains("acme")));
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn write_skill(dir: &Path, name: &str, description: &str) {
    write(
        &dir.join(name).join("SKILL.md"),
        &format!("---\nname: {name}\ndescription: {description}\n---\n"),
    );
}

#[test]
fn listing_keeps_one_project_per_folder_sorted_by_label() {
    let root = crate::paths::scratch_dir("on-n-off-inspect-projects");
    let on_n_off = root.join("a").join("on-n-off");
    let acme = root.join("acme");
    let path = |dir: &Path| dir.to_string_lossy().into_owned();
    let recorded = [
        path(&on_n_off),
        format!("{}{}", path(&on_n_off), std::path::MAIN_SEPARATOR),
        "  ".into(),
        "~/acme".into(),
        path(&acme),
    ];
    write(
        &root.join(".codex").join("config.toml"),
        &recorded
            .iter()
            .map(|key| format!("[projects.'{key}']\ntrust_level = \"trusted\"\n"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let projects = scratch_adapter(AgentId::Codex, &root)
        .projects()
        .list(Some(&root));
    let listed: Vec<_> = projects
        .iter()
        .map(|project| (project.label.as_str(), project.path.clone()))
        .collect();
    assert_eq!(
        listed,
        [("acme", path(&acme)), ("on-n-off", path(&on_n_off))]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_project_view_adds_its_skills_and_mcp_read_only() {
    let home = crate::paths::scratch_dir("on-n-off-project-scope");
    let root = home.join("acme");
    write_skill(&home.join(".claude").join("skills"), "statusline", "Global");
    write_skill(
        &root.join(".claude").join("skills"),
        "local-feed",
        "Project only",
    );
    write(
        &root.join(".mcp.json"),
        r#"{ "mcpServers": { "repo-docs": { "command": "node", "args": ["docs.js"] } } }"#,
    );

    let tab = scratch_adapter(AgentId::Claude, &home)
        .list_scope(Some(&root.to_string_lossy()))
        .unwrap();
    assert_eq!(tab.user_skills.len(), 2);
    let local = tab
        .user_skills
        .iter()
        .find(|skill| skill.name == "local-feed")
        .unwrap();
    assert_eq!(local.origin, ORIGIN_PROJECT);
    assert!(!local.togglable);
    assert!(local.enabled);
    assert_eq!(tab.mcp_servers.len(), 1);
    assert_eq!(tab.mcp_servers[0].name, "repo-docs");
    assert_eq!(tab.mcp_servers[0].origin, ORIGIN_PROJECT);
    assert!(!tab.mcp_servers[0].togglable);
    assert!(tab.mcp_servers[0].id.starts_with("project:"));
    let _ = fs::remove_dir_all(home);
}

#[test]
fn inspect_reads_git_branch_and_local_counts() {
    let home = crate::paths::scratch_dir("on-n-off-inspect-project");
    let root = home.join("acme");
    write_skill(&root.join(".claude").join("skills"), "local-feed", "");
    write(
        &root.join(".mcp.json"),
        r#"{"mcpServers":{"repo-docs":{"command":"node"}}}"#,
    );
    write(&root.join(".git").join("HEAD"), "ref: refs/heads/main\n");
    let inspected = scratch_adapter(AgentId::Claude, &home)
        .projects()
        .inspect(&root, None);
    assert_eq!(inspected.branch, "main");
    assert_eq!(inspected.skill_count, 1);
    assert_eq!(inspected.mcp_count, 1);
    assert_eq!(git_branch(&root), "main");
    let _ = fs::remove_dir_all(home);
}

#[test]
fn inspecting_under_a_home_reads_its_claude_json_and_expands_its_tilde() {
    let home = crate::paths::scratch_dir("on-n-off-inspect-project-home");
    let root = home.join("acme");
    write(&root.join(".git").join("HEAD"), "ref: refs/heads/main\n");
    write(
        &root.join(".mcp.json"),
        r#"{"mcpServers":{"repo-docs":{"command":"node"}}}"#,
    );
    write(
        &home.join(".claude").join(".claude.json"),
        &serde_json::json!({
            "projects": {
                root.to_string_lossy(): {
                    "mcpServers": { "scratchpad": { "command": "node", "args": ["pad.js"] } }
                }
            }
        })
        .to_string(),
    );
    let projects = scratch_adapter(AgentId::Claude, &home).projects();

    let absolute = projects.inspect(&root, Some(&home));
    assert_eq!(
        absolute.mcp_count, 2,
        "the project's own server and the one the home keeps for it"
    );

    let relative = projects.inspect(Path::new("~/acme"), Some(&home));
    assert_eq!(Path::new(&relative.path), root);
    assert_eq!(relative.branch, "main");
    assert_eq!(relative.mcp_count, 2);
    let _ = fs::remove_dir_all(home);
}

#[test]
fn a_project_view_collapses_one_name_across_skill_roots() {
    let home = crate::paths::scratch_dir("on-n-off-project-skill-dedupe");
    let root = home.join("acme");
    write_skill(
        &home.join(".claude").join("skills"),
        "find-skills",
        "User copy",
    );
    write_skill(
        &root.join(".claude").join("skills"),
        "find-skills",
        "Claude copy",
    );
    write_skill(
        &root.join(".agents").join("skills"),
        "find-skills",
        "Agents copy",
    );
    write_skill(
        &root.join(".agents").join("skills"),
        "vercel-react-best-practices",
        "Vercel",
    );
    write_skill(
        &root.join(".claude").join("skills"),
        "vercel-react-best-practices",
        "Vercel again",
    );

    let tab = scratch_adapter(AgentId::Claude, &home)
        .list_scope(Some(&root.to_string_lossy()))
        .unwrap();
    let names: Vec<_> = tab
        .user_skills
        .iter()
        .map(|skill| skill.name.as_str())
        .collect();
    assert_eq!(names, vec!["find-skills", "vercel-react-best-practices"]);
    assert!(tab.user_skills[0].togglable);
    assert_eq!(tab.user_skills[1].origin, ORIGIN_PROJECT);
    assert!(!tab.user_skills[1].togglable);
    let _ = fs::remove_dir_all(home);
}

fn claude_home_with_kit(home: &Path) {
    let root = home.join(".claude");
    let kit = root.join("plugins/cache/market/kit/1.0.0");
    write(
        &kit.join(".mcp.json"),
        r#"{"tracker":{"command":"tracker"}}"#,
    );
    write(
        &root.join("plugins/installed_plugins.json"),
        &serde_json::json!({ "version": 2, "plugins": { "kit@market": [
            { "scope": "user", "installPath": kit, "version": "1.0.0" }
        ] } })
        .to_string(),
    );
    write(
        &root.join("settings.json"),
        r#"{"enabledPlugins":{"kit@market":true}}"#,
    );
}

#[test]
fn a_claude_project_view_follows_its_own_entry() {
    let home = crate::paths::scratch_dir("on-n-off-project-scope-local");
    let root = home.join("acme").join("webapp");
    fs::create_dir_all(&root).unwrap();
    claude_home_with_kit(&home);
    let key = root.to_string_lossy().into_owned();
    write(
        &home.join(".claude").join(".claude.json"),
        &serde_json::json!({
            "mcpServers": { "github": { "command": "npx" } },
            "projects": {
                "/Users/me/elsewhere": {
                    "mcpServers": { "library-docs": { "url": "https://docs.example/mcp" } }
                },
                key.clone(): {
                    "mcpServers": { "scratchpad": { "command": "node", "args": ["pad.js"] } },
                    "disabledMcpServers": ["scratchpad", "plugin:kit:tracker"]
                }
            }
        })
        .to_string(),
    );
    let adapter = scratch_adapter(AgentId::Claude, &home);
    let everywhere: Vec<_> = adapter
        .list_scope(None)
        .unwrap()
        .mcp_servers
        .into_iter()
        .map(|server| server.id)
        .collect();
    assert_eq!(
        everywhere,
        [
            "github",
            "local:library-docs",
            "local:scratchpad",
            "plugin:kit:tracker"
        ]
    );

    let tab = adapter.list_scope(Some(&key)).unwrap();

    let view: Vec<_> = tab
        .mcp_servers
        .iter()
        .map(|server| (server.id.as_str(), server.origin.as_str(), server.enabled))
        .collect();
    assert_eq!(
        view,
        [
            ("github", "", true),
            ("project:scratchpad", ORIGIN_PROJECT, false),
            ("plugin:kit:tracker", "plugin", false),
        ]
    );
    let _ = fs::remove_dir_all(home);
}

#[test]
fn only_a_claude_project_view_reads_the_claude_json_entry() {
    let home = crate::paths::scratch_dir("on-n-off-project-scope-home");
    let project = home.join("acme").join("webapp");
    fs::create_dir_all(&project).unwrap();
    write(
        &home.join(".claude").join(".claude.json"),
        &serde_json::json!({
            "projects": {
                project.to_string_lossy(): {
                    "mcpServers": { "scratchpad": { "command": "node", "args": ["pad.js"] } }
                }
            }
        })
        .to_string(),
    );
    let view = |id| {
        scratch_adapter(id, &home)
            .list_scope(Some(&project.to_string_lossy()))
            .unwrap()
    };

    let claude = view(AgentId::Claude);
    let codex = view(AgentId::Codex);

    let claude_ids: Vec<_> = claude
        .mcp_servers
        .iter()
        .map(|server| server.id.as_str())
        .collect();
    assert_eq!(claude_ids, ["project:scratchpad"]);
    assert_eq!(claude.mcp_servers[0].origin, ORIGIN_PROJECT);
    assert!(codex.mcp_servers.is_empty(), "{:?}", codex.mcp_servers);
    let _ = fs::remove_dir_all(home);
}

#[test]
fn claude_lists_the_projects_its_own_claude_json_names() {
    let home = crate::paths::scratch_dir("on-n-off-claude-projects");
    let webapp = home.join("acme").join("webapp");
    write_skill(&webapp.join(".claude").join("skills"), "local-feed", "");
    write(
        &home.join(".claude").join(".claude.json"),
        &serde_json::json!({
            "projects": {
                webapp.to_string_lossy(): {
                    "mcpServers": { "scratchpad": { "command": "node" } }
                }
            }
        })
        .to_string(),
    );
    let listed: Vec<_> = scratch_adapter(AgentId::Claude, &home)
        .projects()
        .list(None)
        .into_iter()
        .map(|project| (project.label, project.skill_count, project.mcp_count))
        .collect();
    assert_eq!(listed, [("webapp".to_string(), 1, 1)]);
    let _ = fs::remove_dir_all(home);
}

#[test]
fn a_projects_own_mcp_json_wins_over_the_claude_json_entry_of_one_name() {
    let home = crate::paths::scratch_dir("on-n-off-project-mcp-precedence");
    let root = home.join("acme").join("webapp");
    write(
        &root.join(".mcp.json"),
        r#"{"mcpServers":{"scratchpad":{"command":"from-file"}}}"#,
    );
    write(
        &home.join(".claude").join(".claude.json"),
        &serde_json::json!({
            "projects": {
                root.to_string_lossy(): {
                    "mcpServers": { "scratchpad": { "command": "from-entry" } }
                }
            }
        })
        .to_string(),
    );
    let tab = scratch_adapter(AgentId::Claude, &home)
        .list_scope(Some(&root.to_string_lossy()))
        .unwrap();
    let scratchpads: Vec<_> = tab
        .mcp_servers
        .iter()
        .filter(|server| server.id == "project:scratchpad")
        .map(|server| server.source.as_str())
        .collect();
    assert_eq!(scratchpads, ["from-file"]);
    let _ = fs::remove_dir_all(home);
}
