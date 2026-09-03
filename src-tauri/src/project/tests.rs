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
fn reads_claude_inline_project_mcp_servers() {
    let servers = parse_claude_project_mcp(
        r#"{
                "mcpServers": { "github": { "command": "npx" } },
                "projects": {
                    "E:/dev/app": { "mcpServers": { "local-only": { "command": "node" } } }
                }
            }"#,
        Path::new(r"E:\dev\app"),
    );
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].id, "local-only");
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
