use std::fs;
use std::path::{Path, PathBuf};

use crate::adapter::{scratch_adapter, AgentAdapter};
use crate::dto::{AgentId, AgentTabDto};

pub(super) struct Home {
    pub(super) root: PathBuf,
    pub(super) project: PathBuf,
}

impl Home {
    pub(super) fn new(tag: &str) -> Self {
        let root = crate::paths::scratch_dir(&format!("on-n-off-claude-mcp-{tag}"));
        let project = root.join("acme").join("webapp");
        fs::create_dir_all(&project).unwrap();
        Self { root, project }
    }

    pub(super) fn key(&self) -> String {
        self.project.to_string_lossy().into_owned()
    }

    pub(super) fn claude_json(&self) -> PathBuf {
        self.root.join(".claude").join(".claude.json")
    }

    pub(super) fn write(&self, path: &Path, body: serde_json::Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body.to_string()).unwrap();
    }

    pub(super) fn adapter(&self) -> Box<dyn AgentAdapter> {
        scratch_adapter(AgentId::Claude, &self.root)
    }

    pub(super) fn with_kit(&self, servers: serde_json::Value) {
        let claude = self.root.join(".claude");
        let kit = claude.join("plugins/cache/market/kit/1.0.0");
        self.write(&kit.join(".mcp.json"), servers);
        self.write(
            &claude.join("plugins/installed_plugins.json"),
            serde_json::json!({ "version": 2, "plugins": { "kit@market": [
                { "scope": "user", "installPath": kit, "version": "1.0.0" }
            ] } }),
        );
        self.write(
            &claude.join("settings.json"),
            serde_json::json!({ "enabledPlugins": { "kit@market": true } }),
        );
    }

    pub(super) fn seeded(&self, entry: serde_json::Value) -> Box<dyn AgentAdapter> {
        self.with_kit(serde_json::json!({
            "tracker": { "command": "tracker" },
            "search": { "command": "search" }
        }));
        self.write(
            &self.claude_json(),
            serde_json::json!({
                "mcpServers": {
                    "github": { "command": "npx" },
                    "notes": { "command": "notes" }
                },
                "projects": { self.key(): entry }
            }),
        );
        self.write(
            &self.project.join(".mcp.json"),
            serde_json::json!({ "mcpServers": {
                "lint": { "command": "lint" },
                "repo-docs": { "command": "repo-docs" }
            } }),
        );
        self.adapter()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn listed(tab: &AgentTabDto) -> Vec<(String, String, String)> {
    tab.mcp_servers
        .iter()
        .map(|server| {
            (
                server.id.clone(),
                server.source.clone(),
                server.origin.clone(),
            )
        })
        .collect()
}

fn row(id: &str, source: &str, origin: &str) -> (String, String, String) {
    (id.into(), source.into(), origin.into())
}

pub(super) fn states(tab: &AgentTabDto) -> Vec<(String, bool)> {
    tab.mcp_servers
        .iter()
        .map(|server| (server.id.clone(), server.enabled))
        .collect()
}

pub(super) fn state(id: &str, on: bool) -> (String, bool) {
    (id.into(), on)
}

#[test]
fn a_project_view_lists_the_mcp_json_server_first_and_the_users_beside_the_projects() {
    let home = Home::new("listing");
    home.write(
        &home.claude_json(),
        serde_json::json!({
            "mcpServers": {
                "github": { "command": "npx" },
                "docs": { "url": "https://user.example/mcp" }
            },
            "projects": {
                home.key(): {
                    "mcpServers": {
                        "docs": { "command": "entry-docs" },
                        "pad": { "command": "pad" }
                    }
                }
            }
        }),
    );
    home.write(
        &home.project.join(".mcp.json"),
        serde_json::json!({ "mcpServers": {
            "docs": { "command": "repo-docs" },
            "lint": { "command": "lint" }
        } }),
    );
    let adapter = home.adapter();

    let view = adapter.list_scope(Some(&home.key())).unwrap();
    let inspected = adapter.projects().inspect(&home.project, None);

    assert_eq!(
        listed(&view),
        [
            row("docs", "https://user.example/mcp", ""),
            row("project:docs", "repo-docs", "project"),
            row("github", "npx", ""),
            row("project:lint", "lint", "project"),
            row("project:pad", "pad", "project"),
        ]
    );
    assert_eq!(inspected.mcp_count, 3);
}

#[test]
fn a_project_view_turns_off_every_kind_of_server_its_list_names() {
    let home = Home::new("project-list");
    let adapter = home.seeded(serde_json::json!({
        "mcpServers": {
            "pad": { "command": "pad" },
            "scratch": { "command": "scratch" }
        },
        "disabledMcpServers": ["github", "plugin:kit:tracker", "lint", "pad"]
    }));

    let view = adapter.list_scope(Some(&home.key())).unwrap();

    assert_eq!(
        states(&view),
        [
            state("github", false),
            state("project:lint", false),
            state("notes", true),
            state("project:pad", false),
            state("project:repo-docs", true),
            state("project:scratch", true),
            state("plugin:kit:search", true),
            state("plugin:kit:tracker", false),
        ]
    );
}

const SWITCHED_IN_PROJECTS: &str = "Claude Code turns MCP servers off one project at a time, so on-n-off switches them only inside a project. Choose a project under Scope, or use /mcp in Claude Code.";

enum Outcome {
    Listed(&'static [&'static str]),
    OnlyInProjects,
    NotTogglable,
    NotFound,
}

pub(super) fn document(home: &Home) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(home.claude_json()).unwrap()).unwrap()
}

#[test]
fn a_switch_writes_the_projects_own_list_inside_a_recorded_project_and_nowhere_else() {
    use Outcome::{Listed, NotFound, NotTogglable, OnlyInProjects};
    let cases: [(Option<&str>, &str, bool, Outcome); 16] = [
        (None, "github", false, OnlyInProjects),
        (None, "plugin:kit:tracker", false, OnlyInProjects),
        (None, "local:pad", false, OnlyInProjects),
        (None, "acme-missing", false, OnlyInProjects),
        (
            Some("webapp"),
            "github",
            false,
            Listed(&["other", "github"]),
        ),
        (Some("webapp"), "github", true, Listed(&["other"])),
        (
            Some("webapp"),
            "plugin:kit:tracker",
            false,
            Listed(&["other", "plugin:kit:tracker"]),
        ),
        (
            Some("webapp"),
            "plugin:kit:tracker",
            true,
            Listed(&["other"]),
        ),
        (Some("webapp"), "project:lint", false, NotTogglable),
        (Some("webapp"), "project:pad", false, NotTogglable),
        (Some("webapp"), "acme-missing", false, NotFound),
        (Some("notes"), "github", false, Listed(&["github"])),
        (
            Some("notes"),
            "plugin:kit:search",
            false,
            Listed(&["plugin:kit:search"]),
        ),
        (Some("api"), "github", false, NotTogglable),
        (Some("api"), "plugin:kit:tracker", false, NotTogglable),
        (Some("notes"), "github", true, Listed(&[])),
    ];
    for (project, id, on, outcome) in cases {
        let home = Home::new("switch");
        let path = |name: &str| home.root.join("acme").join(name);
        let key = |name: &str| path(name).to_string_lossy().into_owned();
        home.with_kit(serde_json::json!({
            "tracker": { "command": "tracker" },
            "search": { "command": "search" }
        }));
        fs::create_dir_all(path("api")).unwrap();
        home.write(
            &home.project.join(".mcp.json"),
            serde_json::json!({ "mcpServers": { "lint": { "command": "lint" } } }),
        );
        let start = serde_json::json!({
            "numStartups": 3,
            "mcpServers": { "github": { "command": "npx" } },
            "projects": {
                key("webapp"): {
                    "allowedTools": ["Read"],
                    "mcpServers": { "pad": { "command": "pad" } },
                    "disabledMcpServers": ["other"]
                },
                key("notes"): { "allowedTools": [] }
            }
        });
        let mut seeded = start;
        if let (true, Listed(_)) = (on, &outcome) {
            let entry = &mut seeded["projects"][key(project.unwrap())];
            let mut list: Vec<serde_json::Value> = entry
                .get("disabledMcpServers")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            list.push(id.into());
            entry["disabledMcpServers"] = list.into();
        }
        home.write(&home.claude_json(), seeded.clone());
        let adapter = home.adapter();
        let changes = crate::change::Changes::beside(&home.root);
        let view = project.map(key);
        let what = format!("{project:?} {id} on={on}");

        let answer = changes.make(
            &*adapter,
            crate::change::Request::mcp(id, on),
            view.as_deref(),
        );

        let refused = |answer: Result<AgentTabDto, crate::dto::AdapterError>, expected: String| {
            match answer {
                Ok(_) => panic!("{what}: made, and .claude.json reads {}", document(&home)),
                Err(error) => assert_eq!(error.message, expected, "{what}"),
            }
            assert_eq!(document(&home), seeded, "{what} wrote");
        };
        match outcome {
            OnlyInProjects => refused(answer, SWITCHED_IN_PROJECTS.to_string()),
            NotTogglable => refused(answer, format!("mcp server is not togglable: {id}")),
            NotFound => refused(answer, format!("mcp server not found: {id}")),
            Listed(list) => {
                let answer = answer.unwrap_or_else(|error| panic!("{what}: {error:?}"));
                let mut expected = seeded.clone();
                expected["projects"][key(project.unwrap())]["disabledMcpServers"] =
                    serde_json::json!(list);
                assert_eq!(document(&home), expected, "{what}");
                let row = answer
                    .mcp_servers
                    .iter()
                    .find(|server| server.id == id)
                    .unwrap_or_else(|| panic!("{what}: no row"));
                assert_eq!((row.enabled, row.togglable), (on, true), "{what}");
            }
        }
    }
}

#[test]
fn only_the_users_and_the_plugins_servers_in_a_recorded_project_can_be_switched() {
    let home = Home::new("switchable");
    let adapter = home.seeded(serde_json::json!({
        "mcpServers": { "pad": { "command": "pad" } }
    }));
    let unrecorded = home.root.join("acme").join("api");
    fs::create_dir_all(&unrecorded).unwrap();
    let switchable = |view: Option<&str>| -> Vec<String> {
        adapter
            .list_scope(view)
            .unwrap()
            .mcp_servers
            .into_iter()
            .filter(|server| server.togglable)
            .map(|server| server.id)
            .collect()
    };

    assert_eq!(switchable(None), Vec::<String>::new());
    assert_eq!(
        switchable(Some(&home.key())),
        ["github", "notes", "plugin:kit:search", "plugin:kit:tracker"]
    );
    assert_eq!(
        switchable(Some(&unrecorded.to_string_lossy())),
        Vec::<String>::new()
    );
}

#[test]
fn neither_a_disabled_key_nor_a_top_level_list_turns_a_server_off() {
    let home = Home::new("dead-keys");
    home.write(
        &home.claude_json(),
        serde_json::json!({
            "mcpServers": {
                "github": { "command": "npx", "disabled": true },
                "notes": { "command": "notes" }
            },
            "disabledMcpServers": ["notes", "plugin:kit:tracker"],
            "projects": { home.key(): {
                "mcpServers": { "pad": { "command": "pad", "disabled": true } }
            } }
        }),
    );
    home.write(
        &home.project.join(".mcp.json"),
        serde_json::json!({
            "mcpServers": { "lint": { "command": "lint", "disabled": true } },
            "disabledMcpServers": ["lint"]
        }),
    );
    home.with_kit(serde_json::json!({ "tracker": { "command": "tracker" } }));
    let adapter = home.adapter();

    let everywhere = adapter.list_scope(None).unwrap();
    let inside = adapter.list_scope(Some(&home.key())).unwrap();

    assert_eq!(
        states(&everywhere),
        [
            state("github", true),
            state("notes", true),
            state("local:pad", true),
            state("plugin:kit:tracker", true),
        ]
    );
    assert_eq!(
        states(&inside),
        [
            state("github", true),
            state("project:lint", true),
            state("notes", true),
            state("project:pad", true),
            state("plugin:kit:tracker", true),
        ]
    );
}

#[test]
fn a_project_claude_code_has_not_recorded_reads_its_mcp_json_servers_on_too() {
    let home = Home::new("dead-keys-unrecorded");
    home.write(
        &home.claude_json(),
        serde_json::json!({
            "mcpServers": { "github": { "command": "npx", "disabled": true } },
            "projects": {}
        }),
    );
    home.write(
        &home.project.join(".mcp.json"),
        serde_json::json!({
            "mcpServers": {
                "lint": { "command": "lint", "disabled": true },
                "pad": { "command": "pad" }
            },
            "disabledMcpServers": ["pad"]
        }),
    );
    let adapter = home.adapter();

    let inside = adapter.list_scope(Some(&home.key())).unwrap();

    assert_eq!(
        states(&inside),
        [
            state("github", true),
            state("project:lint", true),
            state("project:pad", true),
        ]
    );
}
