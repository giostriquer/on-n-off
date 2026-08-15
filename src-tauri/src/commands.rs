use std::sync::Arc;

use crate::adapter::AgentAdapter;
use crate::dto::{
    AdapterError, AgentId, AgentInfo, AgentTabDto, ProjectDto, UsageSummaryDto, UsageSummaryInput,
};
use crate::flags::FeatureFlags;
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
}

impl AppState {
    pub fn production() -> Self {
        Self {
            claude: Arc::new(crate::claude::ClaudeAdapter::new()),
            codex: Arc::new(crate::codex::CodexAdapter::new()),
            antigravity: Arc::new(crate::antigravity::AntigravityAdapter::new()),
            cursor: Arc::new(crate::cursor::CursorAdapter::new()),
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
pub async fn save_app_settings(settings: AppSettings) -> Result<AppSettings, AdapterError> {
    blocking("save app settings", move || {
        crate::settings::save_settings(settings)
    })
    .await
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
