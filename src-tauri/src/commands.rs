use std::sync::Arc;

use crate::adapter::AgentAdapter;
use crate::dto::{
    AdapterError, AgentId, AgentInfo, AgentTabDto, ProjectDto, UsageSummaryDto, UsageSummaryInput,
};
use crate::flags::FeatureFlags;
use crate::settings::{AppSettings, ProviderDiagnose};

pub struct AppState {
    claude: Arc<dyn AgentAdapter>,
    codex: Arc<dyn AgentAdapter>,
    antigravity: Arc<dyn AgentAdapter>,
}

impl AppState {
    pub fn production() -> Self {
        Self {
            claude: Arc::new(crate::claude::ClaudeAdapter::new()),
            codex: Arc::new(crate::codex::CodexAdapter::new()),
            antigravity: Arc::new(crate::antigravity::AntigravityAdapter::new()),
        }
    }

    fn adapter(&self, id: AgentId) -> &Arc<dyn AgentAdapter> {
        match id {
            AgentId::Claude => &self.claude,
            AgentId::Codex => &self.codex,
            AgentId::Antigravity => &self.antigravity,
        }
    }
}

#[tauri::command]
pub fn list_agents(state: tauri::State<AppState>) -> Vec<AgentInfo> {
    vec![
        state.claude.info(),
        state.codex.info(),
        state.antigravity.info(),
    ]
}

#[tauri::command]
pub fn feature_flags() -> FeatureFlags {
    crate::flags::load_flags()
}

#[tauri::command]
pub fn load_app_settings() -> AppSettings {
    crate::settings::load_settings()
}

#[tauri::command]
pub fn save_app_settings(settings: AppSettings) -> Result<AppSettings, AdapterError> {
    crate::settings::save_settings(settings)
}

#[tauri::command]
pub fn diagnose_providers() -> Vec<ProviderDiagnose> {
    crate::settings::diagnose_all()
}

#[tauri::command]
pub fn list_projects(agent_id: AgentId, state: tauri::State<AppState>) -> Vec<ProjectDto> {
    state.adapter(agent_id).list_projects()
}

#[tauri::command]
pub fn inspect_project(agent_id: AgentId, path: String) -> ProjectDto {
    crate::project::inspect_project(std::path::Path::new(&path), agent_id)
}

#[tauri::command]
pub fn list_plugins(
    agent_id: AgentId,
    project_path: Option<String>,
    state: tauri::State<AppState>,
) -> Result<AgentTabDto, AdapterError> {
    state.adapter(agent_id).list_scope(project_path.as_deref())
}

#[tauri::command]
pub fn set_plugin_enabled(
    agent_id: AgentId,
    plugin_id: String,
    enabled: bool,
    state: tauri::State<AppState>,
) -> Result<AgentTabDto, AdapterError> {
    state.adapter(agent_id).set_plugin_enabled(&plugin_id, enabled)
}

#[tauri::command]
pub fn set_skill_enabled(
    agent_id: AgentId,
    skill_id: String,
    enabled: bool,
    state: tauri::State<AppState>,
) -> Result<AgentTabDto, AdapterError> {
    state.adapter(agent_id).set_skill_enabled(&skill_id, enabled)
}

#[tauri::command]
pub fn set_mcp_enabled(
    agent_id: AgentId,
    mcp_id: String,
    enabled: bool,
    state: tauri::State<AppState>,
) -> Result<AgentTabDto, AdapterError> {
    state.adapter(agent_id).set_mcp_enabled(&mcp_id, enabled)
}

#[tauri::command]
pub fn install_plugin(
    agent_id: AgentId,
    source: String,
    state: tauri::State<AppState>,
) -> Result<AgentTabDto, AdapterError> {
    state.adapter(agent_id).install_plugin(&source)
}

#[tauri::command]
pub fn uninstall_plugin(
    agent_id: AgentId,
    plugin_id: String,
    state: tauri::State<AppState>,
) -> Result<AgentTabDto, AdapterError> {
    state.adapter(agent_id).uninstall_plugin(&plugin_id)
}

#[tauri::command]
pub fn update_plugin(
    agent_id: AgentId,
    plugin_id: String,
    state: tauri::State<AppState>,
) -> Result<AgentTabDto, AdapterError> {
    state.adapter(agent_id).update_plugin(&plugin_id)
}

#[tauri::command]
pub fn refresh(
    agent_id: AgentId,
    project_path: Option<String>,
    state: tauri::State<AppState>,
) -> Result<AgentTabDto, AdapterError> {
    state.adapter(agent_id).list_scope(project_path.as_deref())
}

#[tauri::command]
pub async fn usage_summary(input: UsageSummaryInput) -> Result<UsageSummaryDto, AdapterError> {
    tauri::async_runtime::spawn_blocking(move || crate::usage::read_summary(input))
        .await
        .map_err(|error| AdapterError::message(format!("usage scan worker failed: {error}")))?
}
