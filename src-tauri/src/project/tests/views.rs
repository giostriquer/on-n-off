use std::fs;
use std::path::{Path, PathBuf};

use crate::adapter::{scratch_adapter, AgentAdapter};
use crate::dto::{AgentId, AgentTabDto};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn skill(dir: &Path, folder: &str, description: &str) {
    write(
        &dir.join(folder).join("SKILL.md"),
        &format!("---\nname: {folder}\ndescription: {description}\n---\n"),
    );
}

fn muse_plugin(home: &Path, id: &str, enabled: bool) -> String {
    let package = crate::paths::muse_data_for(home)
        .join("plugins")
        .join("cache")
        .join("local")
        .join(id)
        .join("abc")
        .join("package");
    write(
        &package.join(".muse-plugin").join("plugin.json"),
        &format!(r#"{{"name":"{id}","version":"1.0.0"}}"#),
    );
    skill(
        &package.join("skills"),
        &format!("{id}-review"),
        "Muse plugin",
    );
    serde_json::json!({
        "id": id,
        "version": "1.0.0",
        "enabled": enabled,
        "source": { "path": format!("/srv/acme/{id}") },
        "cache_path": package,
    })
    .to_string()
}

fn seed_home(id: AgentId, home: &Path, project: &Path) {
    match id {
        AgentId::Claude => {
            let root = home.join(".claude");
            let kit = root.join("plugins/cache/market/kit/1.0.0");
            skill(&kit.join("skills"), "kit-skill", "Claude plugin");
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
            skill(&root.join("skills"), "statusline", "Claude user");
            write(
                &root.join(".claude.json"),
                r#"{"mcpServers":{"github":{"command":"npx"}},"projects":{"/Users/me/elsewhere":{"mcpServers":{"pad":{"command":"pad"}}}}}"#,
            );
        }
        AgentId::Codex => {
            let root = home.join(".codex");
            let bench = root.join("plugins/cache/workshop/workbench/0.1.0");
            write(
                &bench.join(".codex-plugin/plugin.json"),
                r#"{"name":"workbench","version":"0.1.0"}"#,
            );
            skill(&bench.join("skills"), "bench-skill", "Codex plugin");
            skill(&root.join("skills"), "loom", "Codex user");
            write(
                &root.join("config.toml"),
                &format!(
                    "[plugins.\"workbench@workshop\"]\nenabled = true\n\n[mcp_servers.github]\ncommand = \"npx\"\n\n[projects.'{}']\ntrust_level = \"trusted\"\n",
                    project.display()
                ),
            );
        }
        AgentId::Antigravity => {
            let gemini = home.join(".gemini");
            let alpha = gemini.join("antigravity-cli/plugins/alpha");
            write(&alpha.join("plugin.json"), r#"{"name":"alpha"}"#);
            skill(&alpha.join("skills"), "alpha-skill", "Antigravity plugin");
            write(
                &gemini.join("config/plugins/beta/plugin.json"),
                r#"{"name":"beta"}"#,
            );
            write(
                &gemini.join("config/mcp_config.json"),
                r#"{"mcpServers":{"github":{"command":"npx"}}}"#,
            );
            skill(
                &gemini.join("antigravity-cli/skills"),
                "solo",
                "Antigravity user",
            );
        }
        AgentId::Cursor => {
            let root = home.join(".cursor");
            let acme = root.join("plugins/local/acme");
            write(&acme.join("plugin.json"), r#"{"name":"acme"}"#);
            skill(&acme.join("skills"), "acme-skill", "Cursor plugin");
            skill(&root.join("skills"), "acme-notes", "Cursor user");
            write(
                &root.join("mcp.json"),
                r#"{"mcpServers":{"github":{"command":"npx"}}}"#,
            );
        }
        AgentId::Muse => {
            let data = crate::paths::muse_data_for(home);
            let config = crate::paths::muse_config_for(home);
            write(
                &data.join("plugins/installed.json"),
                &format!(
                    r#"{{"version":1,"plugins":{{"acme-tools":{},"quiet":{}}}}}"#,
                    muse_plugin(home, "acme-tools", true),
                    muse_plugin(home, "quiet", false)
                ),
            );
            write(
                &config.join("settings.json"),
                r#"{"schema_version":1,"mcpServers":{"docs":{"url":"https://docs.example/mcp"}}}"#,
            );
            skill(&config.join("skills"), "acme-notes", "Muse user");
        }
    }
}

fn seed_project(project: &Path) {
    let claude = project.join(".claude").join("skills");
    skill(&claude, "claude-local", "from .claude");
    skill(&claude, "shared", "from .claude");
    skill(&claude, "statusline", "from .claude");
    let agents = project.join(".agents").join("skills");
    skill(&agents, "agents-local", "from .agents");
    skill(&agents, "shared", "from .agents");
    skill(&agents, "kit-skill", "from .agents");
    write(
        &agents.join("loose-note.md"),
        "---\nname: loose-note\ndescription: from .agents\n---\n",
    );
    let codex = project.join(".codex").join("skills");
    skill(&codex, "codex-local", "from .codex");
    skill(&codex, "shared", "from .codex");
    skill(&claude, "twin", "from .claude");
    skill(&codex, "twin", "from .codex");
    skill(
        &project.join(".cursor").join("skills"),
        "cursor-local",
        "from .cursor",
    );
    write(
        &project.join(".mcp.json"),
        r#"{"mcpServers":{"repo-docs":{"command":"node","args":["docs.js"]}}}"#,
    );
    write(
        &project.join(".codex").join("config.toml"),
        "[mcp_servers.codex-tools]\ncommand = \"codex-tools\"\nenabled = false\n\n[mcp_servers.repo-docs]\ncommand = \"other\"\n",
    );
    write(
        &project.join(".agents").join("mcp_config.json"),
        r#"{"mcpServers":{"agy-search":{"serverUrl":"https://search.example/mcp"}}}"#,
    );
    write(
        &project.join(".cursor").join("mcp.json"),
        r#"{"mcpServers":{"cursor-db":{"command":"db","disabled":true}}}"#,
    );
    let ws = project.join(".agents").join("plugins").join("ws");
    write(&ws.join("plugin.json"), r#"{"name":"ws"}"#);
    skill(&ws.join("skills"), "ws-skill", "workspace plugin");
    write(
        &project
            .join("_agents")
            .join("plugins")
            .join("other")
            .join("plugin.json"),
        r#"{"name":"other"}"#,
    );
}

struct Scratch {
    home: PathBuf,
    project: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let home = crate::paths::scratch_dir(&format!("on-n-off-project-view-{tag}"));
        let project = home.join("acme").join("webapp");
        seed_project(&project);
        Self { home, project }
    }

    fn adapter(&self, id: AgentId) -> Box<dyn AgentAdapter> {
        seed_home(id, &self.home, &self.project);
        scratch_adapter(id, &self.home)
    }

    fn project(&self) -> String {
        self.project.to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn rows(tab: &AgentTabDto) -> Vec<String> {
    let switch = |on: bool, togglable: bool| {
        format!(
            "{}{}",
            if on { "on" } else { "off" },
            if togglable { " switchable" } else { "" }
        )
    };
    let mut out = Vec::new();
    for plugin in &tab.plugins {
        out.push(format!(
            "plugin {} {}",
            plugin.id,
            switch(plugin.enabled, plugin.togglable)
        ));
        for skill in &plugin.skills {
            out.push(format!(
                "  skill {} of {} [{}] {}",
                skill.name,
                skill.plugin_id.as_deref().unwrap_or("-"),
                skill.origin,
                switch(skill.enabled, skill.togglable)
            ));
        }
    }
    for skill in &tab.user_skills {
        out.push(format!(
            "skill {} ({}) [{}] {}",
            skill.name,
            skill.description,
            skill.origin,
            switch(skill.enabled, skill.togglable)
        ));
    }
    for server in &tab.mcp_servers {
        out.push(format!(
            "mcp {} {} [{}] {}",
            server.id,
            server.source,
            server.origin,
            switch(server.enabled, server.togglable)
        ));
    }
    out
}

fn expected(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| line.to_string()).collect()
}

#[test]
fn every_provider_lists_its_own_rows_switchable_only_as_it_allows() {
    let cases: [(AgentId, &[&str]); 5] = [
        (
            AgentId::Claude,
            &[
                "plugin kit@market on switchable",
                "  skill kit-skill of kit@market [] on",
                "skill statusline (Claude user) [] on switchable",
                "mcp github npx [] on switchable",
                "mcp local:pad pad [local] on",
                "mcp plugin:kit:tracker tracker [plugin] on",
            ],
        ),
        (
            AgentId::Codex,
            &[
                "plugin workbench@workshop on switchable",
                "  skill bench-skill of workbench@workshop [] on switchable",
                "skill loom (Codex user) [] on switchable",
                "mcp github npx [] on switchable",
            ],
        ),
        (
            AgentId::Antigravity,
            &[
                "plugin alpha@cli on switchable",
                "  skill alpha-skill of alpha@cli [] on",
                "plugin beta@config on",
                "skill solo (Antigravity user) [] on",
                "mcp github npx [] on switchable",
            ],
        ),
        (
            AgentId::Cursor,
            &[
                "plugin acme@local on",
                "  skill acme-skill of acme@local [] on",
                "skill acme-notes (Cursor user) [] on",
                "mcp github npx [] on",
            ],
        ),
        (
            AgentId::Muse,
            &[
                "plugin acme-tools@local on switchable",
                "  skill acme-tools-review of acme-tools@local [] on switchable",
                "plugin quiet@local off switchable",
                "  skill quiet-review of quiet@local [] off",
                "skill acme-notes (Muse user) [] on switchable",
                "mcp docs https://docs.example/mcp [] on",
            ],
        ),
    ];
    for (id, want) in cases {
        let scratch = Scratch::new(&format!("global-{}", id.key()));
        let adapter = scratch.adapter(id);
        let tab = adapter.list_scope(None).unwrap();
        assert_eq!(rows(&tab), expected(want), "{id:?}");
    }
}

#[test]
fn every_provider_shows_what_a_project_brings_marked_and_never_switched() {
    let cases: [(AgentId, &[&str]); 5] = [
        (
            AgentId::Claude,
            &[
                "plugin kit@market on switchable",
                "  skill kit-skill of kit@market [] on",
                "skill agents-local (from .agents) [project] on",
                "skill claude-local (from .claude) [project] on",
                "skill shared (from .claude) [project] on",
                "skill statusline (Claude user) [] on switchable",
                "skill twin (from .claude) [project] on",
                "mcp github npx [] on switchable",
                "mcp project:repo-docs node docs.js [project] on",
                "mcp plugin:kit:tracker tracker [plugin] on",
            ],
        ),
        (
            AgentId::Codex,
            &[
                "plugin workbench@workshop on switchable",
                "  skill bench-skill of workbench@workshop [] on switchable",
                "skill agents-local (from .agents) [project] on",
                "skill codex-local (from .codex) [project] on",
                "skill kit-skill (from .agents) [project] on",
                "skill loom (Codex user) [] on switchable",
                "skill shared (from .codex) [project] on",
                "skill twin (from .codex) [project] on",
                "mcp project:codex-tools codex-tools [project] off",
                "mcp github npx [] on switchable",
                "mcp project:repo-docs node docs.js [project] on",
            ],
        ),
        (
            AgentId::Antigravity,
            &[
                "plugin alpha@cli on switchable",
                "  skill alpha-skill of alpha@cli [] on",
                "plugin beta@config on",
                "plugin project:other@workspace on",
                "plugin project:ws@workspace on",
                "  skill ws-skill of ws@workspace [project] on",
                "skill agents-local (from .agents) [project] on",
                "skill kit-skill (from .agents) [project] on",
                "skill loose-note (from .agents) [project] on",
                "skill shared (from .agents) [project] on",
                "skill solo (Antigravity user) [] on",
                "mcp project:agy-search https://search.example/mcp [project] on",
                "mcp github npx [] on switchable",
                "mcp project:repo-docs node docs.js [project] on",
            ],
        ),
        (
            AgentId::Cursor,
            &[
                "plugin acme@local on",
                "  skill acme-skill of acme@local [] on",
                "skill acme-notes (Cursor user) [] on",
                "skill cursor-local (from .cursor) [project] on",
                "mcp project:cursor-db db [project] off",
                "mcp github npx [] on",
                "mcp project:repo-docs node docs.js [project] on",
            ],
        ),
        (
            AgentId::Muse,
            &[
                "plugin acme-tools@local on switchable",
                "  skill acme-tools-review of acme-tools@local [] on switchable",
                "plugin quiet@local off switchable",
                "  skill quiet-review of quiet@local [] off",
                "skill acme-notes (Muse user) [] on switchable",
                "skill agents-local (from .agents) [project] on",
                "skill claude-local (from .claude) [project] on",
                "skill codex-local (from .codex) [project] on",
                "skill kit-skill (from .agents) [project] on",
                "skill shared (from .agents) [project] on",
                "skill statusline (from .claude) [project] on",
                "skill twin (from .claude) [project] on",
                "mcp docs https://docs.example/mcp [] on",
                "mcp project:repo-docs node docs.js [project] on",
            ],
        ),
    ];
    for (id, want) in cases {
        let scratch = Scratch::new(&format!("project-{}", id.key()));
        let adapter = scratch.adapter(id);
        let tab = adapter.list_scope(Some(&scratch.project())).unwrap();
        assert_eq!(rows(&tab), expected(want), "{id:?}");
        for skill in tab
            .user_skills
            .iter()
            .filter(|skill| skill.origin == super::ORIGIN_PROJECT)
        {
            assert!(skill.id.starts_with("project:"), "{id:?} {}", skill.id);
        }
    }
}

#[test]
fn no_row_is_switchable_where_its_provider_refuses_the_switch() {
    for id in AgentId::ALL {
        let scratch = Scratch::new(&format!("refused-{}", id.key()));
        let adapter = scratch.adapter(id);
        let declared = adapter.capabilities();
        let refused = |change: &crate::capabilities::Change| change.is_refused();
        for view in [None, Some(scratch.project())] {
            let tab = adapter.list_scope(view.as_deref()).unwrap();
            let switchable = |kind: &str| -> Vec<String> {
                match kind {
                    "plugin" => tab
                        .plugins
                        .iter()
                        .filter(|plugin| plugin.togglable)
                        .map(|plugin| plugin.id.clone())
                        .collect(),
                    "skill" => tab
                        .plugins
                        .iter()
                        .flat_map(|plugin| plugin.skills.iter())
                        .chain(tab.user_skills.iter())
                        .filter(|skill| skill.togglable)
                        .map(|skill| skill.name.clone())
                        .collect(),
                    _ => tab
                        .mcp_servers
                        .iter()
                        .filter(|server| server.togglable)
                        .map(|server| server.id.clone())
                        .collect(),
                }
            };
            for (kind, change) in [
                ("plugin", &declared.plugin_switch),
                ("skill", &declared.skill_switch),
                ("mcp", &declared.mcp_switch),
            ] {
                if refused(change) {
                    assert_eq!(switchable(kind), Vec::<String>::new(), "{id:?} {kind}");
                }
            }
        }
    }
}

#[test]
fn every_provider_reads_the_projects_mcp_json() {
    for id in AgentId::ALL {
        let scratch = Scratch::new(&format!("mcp-json-{}", id.key()));
        let adapter = scratch.adapter(id);
        let tab = adapter.list_scope(Some(&scratch.project())).unwrap();
        let server = tab
            .mcp_servers
            .iter()
            .find(|server| server.id == "project:repo-docs")
            .unwrap_or_else(|| panic!("{id:?} does not read .mcp.json"));
        assert_eq!(server.source, "node docs.js", "{id:?}");
        assert_eq!(server.origin, super::ORIGIN_PROJECT, "{id:?}");
        assert!(!server.togglable, "{id:?}");
    }
}

#[test]
fn inspecting_a_project_counts_what_each_provider_reads_there() {
    let cases = [
        (AgentId::Claude, 6, 1),
        (AgentId::Codex, 5, 2),
        (AgentId::Antigravity, 4, 2),
        (AgentId::Cursor, 1, 2),
        (AgentId::Muse, 7, 1),
    ];
    for (id, skills, servers) in cases {
        let scratch = Scratch::new(&format!("inspect-{}", id.key()));
        let inspected = scratch
            .adapter(id)
            .projects()
            .inspect(&scratch.project, None);
        assert_eq!(
            (inspected.skill_count, inspected.mcp_count),
            (skills, servers),
            "{id:?}"
        );
        assert_eq!(inspected.label, "webapp");
        assert_eq!(Path::new(&inspected.path), scratch.project);
    }
}

#[test]
fn codex_lists_the_projects_its_config_trusts() {
    let scratch = Scratch::new("codex-projects");
    let adapter = scratch.adapter(AgentId::Codex);
    let listed: Vec<_> = adapter
        .projects()
        .list(None)
        .into_iter()
        .map(|project| {
            (
                project.label,
                project.path,
                project.skill_count,
                project.mcp_count,
            )
        })
        .collect();
    assert_eq!(listed, [("webapp".to_string(), scratch.project(), 5, 2)]);
}
