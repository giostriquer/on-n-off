use super::*;

#[cfg(test)]
pub fn projects_from_paths(paths: Vec<String>) -> Vec<ProjectDto> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            continue;
        }
        let id = normalize_project_key(trimmed);
        if !seen.insert(id.clone()) {
            continue;
        }
        out.push(ProjectDto {
            id,
            label: project_label(trimmed),
            path: trimmed.to_string(),
            branch: String::new(),
            skill_count: 0,
            mcp_count: 0,
        });
    }
    out.sort_by(|a, b| {
        a.label
            .to_ascii_lowercase()
            .cmp(&b.label.to_ascii_lowercase())
            .then_with(|| {
                a.path
                    .to_ascii_lowercase()
                    .cmp(&b.path.to_ascii_lowercase())
            })
    });
    out
}

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
        "[projects.'E:\\dev\\on-n-off']\ntrust_level = \"trusted\"\n\n[projects.'E:\\dev\\conoswiki']\ntrust_level = \"trusted\"\n",
    );
    assert!(codex.iter().any(|path| path.contains("on-n-off")));
    assert!(codex.iter().any(|path| path.contains("conoswiki")));
    let projects = projects_from_paths(vec![
        r"E:\dev\on-n-off".into(),
        r"E:\dev\on-n-off\".into(),
        r"E:\dev\conoswiki".into(),
    ]);
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].label, "conoswiki");
    assert_eq!(projects[1].label, "on-n-off");
}

#[test]
fn overlays_project_skills_and_mcp_read_only() {
    let root = crate::paths::scratch_dir("on-n-off-project-scope");
    fs::create_dir_all(root.join(".claude").join("skills").join("local-feed")).unwrap();
    fs::write(
        root.join(".claude")
            .join("skills")
            .join("local-feed")
            .join("SKILL.md"),
        "---\nname: local-feed\ndescription: Project only\n---\n",
    )
    .unwrap();
    fs::write(
        root.join(".mcp.json"),
        r#"{ "mcpServers": { "repo-docs": { "command": "node", "args": ["docs.js"] } } }"#,
    )
    .unwrap();

    let mut tab = AgentTabDto {
        plugins: vec![],
        user_skills: vec![SkillDto {
            id: "statusline".into(),
            plugin_id: None,
            name: "statusline".into(),
            description: "Global".into(),
            enabled: true,
            togglable: true,
            origin: String::new(),
        }],
        mcp_servers: vec![],
        hooks: vec![],
    };
    overlay_project(&mut tab, &root, AgentId::Claude);
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
}

#[test]
fn inspect_reads_git_branch_and_local_counts() {
    let root = crate::paths::scratch_dir("on-n-off-inspect-project");
    fs::create_dir_all(root.join(".claude").join("skills").join("local-feed")).unwrap();
    fs::write(
        root.join(".claude")
            .join("skills")
            .join("local-feed")
            .join("SKILL.md"),
        "---\nname: local-feed\n---\n",
    )
    .unwrap();
    fs::write(
        root.join(".mcp.json"),
        r#"{"mcpServers":{"repo-docs":{"command":"node"}}}"#,
    )
    .unwrap();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
    let inspected = inspect_project(&root, AgentId::Claude);
    assert_eq!(inspected.branch, "main");
    assert_eq!(inspected.skill_count, 1);
    assert_eq!(inspected.mcp_count, 1);
    assert_eq!(git_branch(&root), "main");
    let expanded = expand_project_path("~/dev/app");
    assert!(
        expanded
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("dev/app"),
        "{expanded:?}"
    );
}

fn write_skill(dir: &Path, name: &str, description: &str) {
    fs::create_dir_all(dir.join(name)).unwrap();
    fs::write(
        dir.join(name).join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n"),
    )
    .unwrap();
}

#[test]
fn overlay_collapses_same_name_across_skill_roots() {
    let root = crate::paths::scratch_dir("on-n-off-project-skill-dedupe");
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

    let mut tab = AgentTabDto {
        plugins: vec![],
        user_skills: vec![SkillDto {
            id: "find-skills".into(),
            plugin_id: None,
            name: "find-skills".into(),
            description: "User copy".into(),
            enabled: true,
            togglable: true,
            origin: String::new(),
        }],
        mcp_servers: vec![],
        hooks: vec![],
    };
    overlay_project(&mut tab, &root, AgentId::Claude);
    let names: Vec<_> = tab
        .user_skills
        .iter()
        .map(|skill| skill.name.as_str())
        .collect();
    assert_eq!(names, vec!["find-skills", "vercel-react-best-practices"]);
    assert!(tab.user_skills[0].togglable);
    assert_eq!(tab.user_skills[1].origin, ORIGIN_PROJECT);
    assert!(!tab.user_skills[1].togglable);
}

/// Inside a Claude project, that project's `~/.claude.json` entry decides: its servers become
/// project rows (off when its `disabledMcpServers` names them), a plugin server it switched off
/// reads off, and the rows standing for servers kept for particular projects are not repeated.
#[test]
fn a_claude_project_view_follows_its_own_entry() {
    let root = crate::paths::scratch_dir("on-n-off-project-scope-local");
    let key = root.to_string_lossy().into_owned();
    let config = serde_json::json!({
        "projects": {
            key: {
                "mcpServers": { "scratchpad": { "command": "node", "args": ["pad.js"] } },
                "disabledMcpServers": ["scratchpad", "plugin:kit:tracker"]
            }
        }
    });
    let row = |id: &str, origin: &str| crate::dto::McpServerDto {
        id: id.into(),
        name: id.rsplit(':').next().unwrap().into(),
        system: "http".into(),
        source: "https://docs.example/mcp".into(),
        enabled: true,
        togglable: origin.is_empty(),
        origin: origin.into(),
        plugin_id: None,
        projects: Vec::new(),
    };
    let mut tab = AgentTabDto {
        plugins: vec![],
        user_skills: vec![],
        mcp_servers: vec![
            row("local:library-docs", "local"),
            row("github", ""),
            row("plugin:kit:tracker", "plugin"),
        ],
        hooks: vec![],
    };

    overlay_project_with(&mut tab, &root, AgentId::Claude, &config);

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
    let _ = fs::remove_dir_all(root);
}

/// The project views read the home's `~/.claude.json` for Claude and for no one else: the same
/// home, whose entry for this project keeps one server, gives Claude a project row and Codex none.
#[test]
fn only_a_claude_project_view_reads_the_homes_claude_json() {
    let home = crate::paths::scratch_dir("on-n-off-project-scope-home");
    let project = home.join("acme").join("webapp");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        home.join(".claude.json"),
        serde_json::json!({
            "projects": {
                project.to_string_lossy(): {
                    "mcpServers": { "scratchpad": { "command": "node", "args": ["pad.js"] } }
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let empty = || AgentTabDto {
        plugins: vec![],
        user_skills: vec![],
        mcp_servers: vec![],
        hooks: vec![],
    };

    let mut claude = empty();
    overlay_project_in(&mut claude, &project, AgentId::Claude, Some(&home));
    let mut codex = empty();
    overlay_project_in(&mut codex, &project, AgentId::Codex, Some(&home));

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
