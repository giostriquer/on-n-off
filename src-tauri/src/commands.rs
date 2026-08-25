use std::sync::Arc;

use crate::adapter::AgentAdapter;
use crate::dto::{
    AdapterError, AgentId, AgentInfo, AgentTabDto, GithubPrsDto, InstallItemsRequest,
    InstallItemsResultDto, ItemStatusDto, MarketplaceInspectDto, ProjectDto, ProviderLimitsDto,
    UpdateItemMode, UsageSummaryDto, UsageSummaryInput,
};
use crate::flags::FeatureFlags;
use crate::item_install::ItemService;
use crate::settings::{AppSettings, ProviderDiagnose};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterBuildInfo {
    enabled: bool,
    installer_kind: Option<&'static str>,
    target: Option<&'static str>,
}

pub struct AppState {
    claude: Arc<dyn AgentAdapter>,
    codex: Arc<dyn AgentAdapter>,
    antigravity: Arc<dyn AgentAdapter>,
    cursor: Arc<dyn AgentAdapter>,
    /// Local skill/agent installs; `Err` only when no home directory could be resolved.
    items: Result<Arc<ItemService>, AdapterError>,
}

impl AppState {
    pub fn production() -> Self {
        Self {
            claude: Arc::new(crate::claude::ClaudeAdapter::new()),
            codex: Arc::new(crate::codex::CodexAdapter::new()),
            antigravity: Arc::new(crate::antigravity::AntigravityAdapter::new()),
            cursor: Arc::new(crate::cursor::CursorAdapter::new()),
            items: ItemService::production().map(Arc::new),
        }
    }

    fn adapter(&self, id: AgentId) -> Arc<dyn AgentAdapter> {
        match id {
            AgentId::Claude => Arc::clone(&self.claude),
            AgentId::Codex => Arc::clone(&self.codex),
            AgentId::Antigravity => Arc::clone(&self.antigravity),
            AgentId::Cursor => Arc::clone(&self.cursor),
        }
    }

    fn items(&self) -> Result<Arc<ItemService>, AdapterError> {
        self.items.clone()
    }
}

async fn blocking<T, F>(operation: &'static str, run: F) -> Result<T, AdapterError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AdapterError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(run)
        .await
        .map_err(|error| AdapterError::message(format!("{operation} worker failed: {error}")))?
}

#[tauri::command]
pub async fn inspect_marketplace(
    owner: String,
    repo: String,
    git_ref: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<MarketplaceInspectDto, AdapterError> {
    let items = state.items()?;
    blocking("marketplace inspect", move || {
        items.inspect_marketplace(&owner, &repo, git_ref.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn install_items(
    request: InstallItemsRequest,
    state: tauri::State<'_, AppState>,
) -> Result<InstallItemsResultDto, AdapterError> {
    let items = state.items()?;
    let adapters: Vec<(AgentId, Arc<dyn AgentAdapter>)> = request
        .targets
        .iter()
        .map(|target| (target.provider, state.adapter(target.provider)))
        .collect();
    blocking("item install", move || {
        let resolve = |target: &crate::dto::ItemTarget| {
            adapters
                .iter()
                .find(|(provider, _)| *provider == target.provider)
                .ok_or_else(|| AdapterError::message("unknown provider"))
                .and_then(|(_, adapter)| adapter.item_roots(&target.scope))
        };
        items.install_items(request, &resolve)
    })
    .await
}

#[tauri::command]
pub async fn item_update_status(
    provider: AgentId,
    project_path: Option<String>,
    force: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ItemStatusDto>, AdapterError> {
    let items = state.items()?;
    blocking("item status", move || {
        items.item_update_status(provider, project_path.as_deref(), force)
    })
    .await
}

#[tauri::command]
pub async fn update_item(
    id: String,
    mode: UpdateItemMode,
    state: tauri::State<'_, AppState>,
) -> Result<ItemStatusDto, AdapterError> {
    let items = state.items()?;
    blocking("item update", move || items.update_item(&id, mode)).await
}

#[tauri::command]
pub async fn remove_item(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), AdapterError> {
    let items = state.items()?;
    blocking("item remove", move || items.remove_item(&id)).await
}

/// Opens an item's upstream page in the default browser; only github.com links are accepted.
#[tauri::command]
pub async fn open_url(url: String, app: tauri::AppHandle) -> Result<(), AdapterError> {
    use tauri_plugin_opener::OpenerExt;
    if !crate::item_install::is_openable_url(&url) {
        return Err(AdapterError::message(format!("refusing to open {url}")));
    }
    blocking("open url", move || {
        app.opener()
            .open_url(&url, None::<&str>)
            .map_err(|error| AdapterError::message(error.to_string()))
    })
    .await
}

#[tauri::command]
pub async fn list_agents(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AgentInfo>, AdapterError> {
    let adapters = [
        Arc::clone(&state.claude),
        Arc::clone(&state.codex),
        Arc::clone(&state.antigravity),
        Arc::clone(&state.cursor),
    ];
    blocking("provider health", move || {
        Ok(adapters.into_iter().map(|adapter| adapter.info()).collect())
    })
    .await
}

#[tauri::command]
pub async fn feature_flags() -> Result<FeatureFlags, AdapterError> {
    blocking("feature flags", || Ok(crate::flags::load_flags())).await
}

#[tauri::command]
pub fn updater_build_info() -> UpdaterBuildInfo {
    let installer_kind = option_env!("ON_N_OFF_UPDATER_KIND");
    let target = option_env!("ON_N_OFF_UPDATER_TARGET");
    UpdaterBuildInfo {
        enabled: installer_kind.is_some() && target.is_some(),
        installer_kind,
        target,
    }
}

#[tauri::command]
pub async fn load_app_settings() -> Result<AppSettings, AdapterError> {
    blocking("app settings", || Ok(crate::settings::load_settings())).await
}

#[tauri::command]
pub async fn save_app_settings(
    settings: AppSettings,
    app: tauri::AppHandle,
) -> Result<AppSettings, AdapterError> {
    let saved = blocking("save app settings", move || {
        crate::settings::save_settings(settings)
    })
    .await?;
    crate::limits_monitor::wake(&app);
    crate::github_monitor::wake(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn request_notification_permission(app: tauri::AppHandle) -> Result<bool, AdapterError> {
    crate::notifications::request_permission(app).await
}

#[tauri::command]
pub async fn diagnose_providers() -> Result<Vec<ProviderDiagnose>, AdapterError> {
    blocking("provider diagnostics", || {
        Ok(crate::settings::diagnose_all())
    })
    .await
}

#[tauri::command]
pub async fn list_projects(
    agent_id: AgentId,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProjectDto>, AdapterError> {
    let adapter = state.adapter(agent_id);
    blocking("project discovery", move || Ok(adapter.list_projects())).await
}

#[tauri::command]
pub async fn inspect_project(agent_id: AgentId, path: String) -> Result<ProjectDto, AdapterError> {
    blocking("project inspection", move || {
        Ok(crate::project::inspect_project(
            std::path::Path::new(&path),
            agent_id,
        ))
    })
    .await
}

#[tauri::command]
pub async fn list_plugins(
    agent_id: AgentId,
    project_path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<AgentTabDto, AdapterError> {
    let adapter = state.adapter(agent_id);
    blocking("catalog scan", move || {
        adapter.list_scope(project_path.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn list_local_plugins(
    agent_id: AgentId,
    project_path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<AgentTabDto, AdapterError> {
    let adapter = state.adapter(agent_id);
    blocking("local catalog scan", move || {
        adapter.list_local_scope(project_path.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn set_plugin_enabled(
    agent_id: AgentId,
    plugin_id: String,
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> Result<AgentTabDto, AdapterError> {
    let adapter = state.adapter(agent_id);
    blocking("plugin toggle", move || {
        adapter.set_plugin_enabled(&plugin_id, enabled)
    })
    .await
}

#[tauri::command]
pub async fn set_skill_enabled(
    agent_id: AgentId,
    skill_id: String,
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> Result<AgentTabDto, AdapterError> {
    let adapter = state.adapter(agent_id);
    blocking("skill toggle", move || {
        adapter.set_skill_enabled(&skill_id, enabled)
    })
    .await
}

#[tauri::command]
pub async fn set_mcp_enabled(
    agent_id: AgentId,
    mcp_id: String,
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> Result<AgentTabDto, AdapterError> {
    let adapter = state.adapter(agent_id);
    blocking("MCP toggle", move || {
        adapter.set_mcp_enabled(&mcp_id, enabled)
    })
    .await
}

#[tauri::command]
pub async fn install_plugin(
    agent_id: AgentId,
    source: String,
    state: tauri::State<'_, AppState>,
) -> Result<AgentTabDto, AdapterError> {
    let adapter = state.adapter(agent_id);
    blocking("plugin install", move || adapter.install_plugin(&source)).await
}

#[tauri::command]
pub async fn uninstall_plugin(
    agent_id: AgentId,
    plugin_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<AgentTabDto, AdapterError> {
    let adapter = state.adapter(agent_id);
    blocking("plugin uninstall", move || {
        adapter.uninstall_plugin(&plugin_id)
    })
    .await
}

#[tauri::command]
pub async fn update_plugin(
    agent_id: AgentId,
    plugin_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<AgentTabDto, AdapterError> {
    let adapter = state.adapter(agent_id);
    blocking("plugin update", move || adapter.update_plugin(&plugin_id)).await
}

#[tauri::command]
pub async fn refresh(
    agent_id: AgentId,
    project_path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<AgentTabDto, AdapterError> {
    let adapter = state.adapter(agent_id);
    blocking("catalog refresh", move || {
        adapter.list_scope(project_path.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn usage_summary(input: UsageSummaryInput) -> Result<UsageSummaryDto, AdapterError> {
    blocking("usage scan", move || crate::usage::read_summary(input)).await
}

/// Live subscription rate limits for one provider (Claude Keychain + HTTPS, or Codex app-server)
/// followed by remembered snapshots of its other accounts, off the UI thread. Provider-side
/// problems come back as a `status` on the DTO, not as an `Err`. `force` requests fresh provider
/// authentication through the provider-owned path.
#[tauri::command]
pub async fn read_limits(
    agent_id: AgentId,
    force: bool,
) -> Result<Vec<ProviderLimitsDto>, AdapterError> {
    blocking("limits read", move || {
        Ok(crate::limits::read_limits(agent_id, force))
    })
    .await
}

/// Drop one remembered account snapshot (the "Forget" action on a stale card).
#[tauri::command]
pub async fn forget_limits_snapshot(
    agent_id: AgentId,
    account_id: String,
) -> Result<(), AdapterError> {
    blocking("limits forget", move || {
        crate::limits::forget_snapshot(agent_id, &account_id).map_err(AdapterError::message)
    })
    .await
}

/// The GitHub screen's pull requests (authored, review-requested, assigned) with their CI
/// rollups, off the UI thread. Auth is borrowed from `gh auth token`; GitHub-side problems come
/// back as a `status` + `hint` on the DTO, not as an `Err`. `force` skips the in-memory result.
#[tauri::command]
pub async fn read_github_prs(force: bool) -> Result<GithubPrsDto, AdapterError> {
    blocking("github read", move || Ok(crate::github::read_prs(force))).await
}

#[tauri::command]
pub fn hide_limits_popover(app: tauri::AppHandle) -> Result<(), AdapterError> {
    crate::tray::hide_limits_popover(&app).map_err(AdapterError::message)
}

#[tauri::command]
pub fn open_limits_window(app: tauri::AppHandle) -> Result<(), AdapterError> {
    crate::tray::open_limits_window(&app).map_err(AdapterError::message)
}

#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    crate::tray::quit_app(&app);
}
