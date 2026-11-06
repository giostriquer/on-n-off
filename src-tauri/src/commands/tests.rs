use super::*;

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
    home: std::path::PathBuf,
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
            items: Err(AdapterError::message("no item service in this test")),
        };
        Self { home, state }
    }

    fn write(&self, relative: impl AsRef<std::path::Path>, text: &str) {
        let path = self.home.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

impl Drop for ScratchState {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
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
                .codex
                .list_local_scope(scope.as_deref())
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
