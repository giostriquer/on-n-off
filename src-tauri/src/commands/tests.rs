use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;
use crate::drift::{with_fetch_text, Transient};

const MARKETPLACE: &str =
    "https://raw.githubusercontent.com/acme/workshop/main/.claude-plugin/marketplace.json";

thread_local! {
    static ASKED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static CLOCK: Cell<Option<Instant>> = const { Cell::new(None) };
}

fn ticking() -> Instant {
    CLOCK.with(|clock| {
        let now = clock.get().unwrap_or_else(Instant::now) + Duration::from_millis(1);
        clock.set(Some(now));
        now
    })
}

fn github(url: &str) -> Result<Option<String>, Transient> {
    ASKED.with(|asked| asked.borrow_mut().push(url.to_string()));
    Ok((url == MARKETPLACE)
        .then(|| r#"{"plugins":[{"name":"workbench","version":"2.0.0"}]}"#.to_string()))
}

#[test]
fn each_provider_reaches_its_own_adapter() {
    let state = AppState::production();
    for id in AgentId::ALL {
        assert_eq!(AgentInfo::of(state.adapter(id).as_ref()).id, id);
    }
}

#[test]
fn an_adapter_offers_saved_accounts_exactly_when_the_catalogue_says_so() {
    let state = AppState::production();
    for id in AgentId::ALL {
        assert_eq!(
            state.adapter(id).supports_accounts(),
            id.facts().saved_accounts,
            "{id:?}"
        );
    }
}

#[test]
fn a_category_is_offered_exactly_to_providers_with_limits_accounts() {
    let refusal = "This provider has no Limits accounts to name.";
    for id in AgentId::ALL {
        let answer =
            tauri::async_runtime::block_on(set_account_category(id, "acct".into(), "Work".into()));
        let refused = answer.as_ref().err().map(|error| error.message.as_str()) == Some(refusal);
        assert_eq!(refused, id.facts().limits.is_none(), "{id:?}");
    }
}

struct ScratchState {
    home: PathBuf,
    state: AppState,
}

impl ScratchState {
    fn new() -> Self {
        let home = crate::paths::scratch_dir("on-n-off-commands");
        let adapter =
            |id| -> Arc<dyn AgentAdapter> { Arc::from(crate::adapter::scratch_adapter(id, &home)) };
        let state = AppState {
            claude: adapter(AgentId::Claude),
            codex: adapter(AgentId::Codex),
            antigravity: adapter(AgentId::Antigravity),
            cursor: adapter(AgentId::Cursor),
            muse: adapter(AgentId::Muse),
            changes: Arc::new(Changes::at(home.join("_backups"))),
            upstreams: Arc::new(Memo::on(ticking)),
            items: Err(AdapterError::message("no item service in this test")),
        };
        Self { home, state }
    }

    fn write(&self, relative: impl AsRef<Path>, text: &str) {
        let path = self.home.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn with_claude_marketplace() -> Self {
        let scratch = Self::new();
        let root = scratch.home.join(".claude");
        let marketplace = root.join("plugins/marketplaces/workshop");
        scratch.write(
            marketplace.join(".claude-plugin/marketplace.json"),
            r#"{"plugins":[{"name":"workbench","version":"1.1.0","source":"./plugins/workbench"}]}"#,
        );
        scratch.write(
            root.join("plugins/known_marketplaces.json"),
            &serde_json::json!({ "workshop": {
                "source": { "repo": "acme/workshop" },
                "installLocation": marketplace,
            } })
            .to_string(),
        );
        let install = root.join("plugins/cache/workshop/workbench/1.0.0");
        fs::create_dir_all(&install).unwrap();
        scratch.write(
            root.join("plugins/installed_plugins.json"),
            &serde_json::json!({ "version": 2, "plugins": {
                "workbench@workshop": [{ "scope": "user", "installPath": install, "version": "1.0.0" }],
            } })
            .to_string(),
        );
        scratch.write(root.join("settings.json"), "{}");
        scratch
    }

    fn read_catalog(
        &self,
        command: fn(&AppState, AgentId, Option<&str>) -> Result<AgentTabDto, AdapterError>,
    ) -> (String, Vec<String>) {
        ASKED.with(|asked| asked.borrow_mut().clear());
        let tab = with_fetch_text(github, || command(&self.state, AgentId::Claude, None)).unwrap();
        let upstream = tab
            .plugins
            .iter()
            .find(|plugin| plugin.id == "workbench@workshop")
            .map(|plugin| plugin.upstream.clone())
            .unwrap_or_default();
        let asked = ASKED.with(|asked| asked.borrow_mut().drain(..).collect());
        (upstream, asked)
    }
}

impl Drop for ScratchState {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

#[test]
fn a_change_reaches_the_named_provider_and_answers_the_scope_it_names() {
    let scratch = ScratchState::new();
    scratch.write(
        ".codex/config.toml",
        "[mcp_servers.github]\ncommand = \"npx\"\n",
    );
    let project = scratch.home.join("acme-project");
    let roots = scratch
        .state
        .codex
        .item_roots(&crate::dto::ItemScope::Project {
            project_path: project.to_string_lossy().into_owned(),
        })
        .unwrap();
    scratch.write(
        roots.skills.join("acme-local/SKILL.md"),
        "---\nname: acme-local\ndescription: Acme\n---\n",
    );
    let project = Some(project.to_string_lossy().into_owned());

    for (scope, on) in [(None, false), (project.clone(), true), (project, false)] {
        let request = Request::SwitchMcp {
            id: "github".into(),
            on,
        };
        let answer = tauri::async_runtime::block_on(change(
            &scratch.state,
            AgentId::Codex,
            request,
            scope.clone(),
            "MCP toggle",
        ))
        .unwrap();
        let what = format!("{scope:?} on={on}");
        assert_eq!(
            answer,
            scratch
                .state
                .local_plugins(AgentId::Codex, scope.as_deref())
                .unwrap(),
            "{what}"
        );
        let github = answer
            .mcp_servers
            .iter()
            .find(|server| server.id == "github");
        assert_eq!(github.map(|server| server.enabled), Some(on), "{what}");
        assert_eq!(
            answer
                .user_skills
                .iter()
                .any(|skill| skill.name == "acme-local"),
            scope.is_some(),
            "{what}"
        );
    }

    let refused = tauri::async_runtime::block_on(change(
        &scratch.state,
        AgentId::Cursor,
        Request::SwitchMcp {
            id: "github".into(),
            on: false,
        },
        None,
        "MCP toggle",
    ))
    .unwrap_err();
    assert_eq!(
        refused.message,
        scratch
            .state
            .cursor
            .capabilities()
            .mcp_switch
            .refusal()
            .message
    );
}

fn asked_once() -> Vec<String> {
    vec![MARKETPLACE.to_string()]
}

#[test]
fn a_local_catalog_read_never_asks_github() {
    let catalogs = ScratchState::with_claude_marketplace();
    let local = catalogs.read_catalog(AppState::local_plugins);
    assert_eq!(local, ("1.1.0".into(), Vec::new()));
    catalogs.read_catalog(AppState::remote_plugins);
    assert_eq!(catalogs.read_catalog(AppState::local_plugins), local);
}

#[test]
fn two_catalog_scans_ask_github_once_through_the_shared_memo() {
    let catalogs = ScratchState::with_claude_marketplace();
    assert_eq!(
        catalogs.read_catalog(AppState::remote_plugins),
        ("2.0.0".into(), asked_once())
    );
    assert_eq!(
        catalogs.read_catalog(AppState::remote_plugins),
        ("2.0.0".into(), Vec::new())
    );
}

#[test]
fn a_catalog_refresh_after_a_scan_asks_github_again() {
    let catalogs = ScratchState::with_claude_marketplace();
    catalogs.read_catalog(AppState::remote_plugins);
    assert_eq!(
        catalogs.read_catalog(AppState::forced_plugins),
        ("2.0.0".into(), asked_once())
    );
    assert_eq!(
        catalogs.read_catalog(AppState::remote_plugins),
        ("2.0.0".into(), Vec::new())
    );
}
