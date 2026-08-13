use std::sync::Arc;

use crate::adapter::AgentAdapter;
use crate::dto::{AdapterError, AgentId, AgentInfo, AgentTabDto};

pub struct AppState {
    claude: Arc<dyn AgentAdapter>,
    codex: Arc<dyn AgentAdapter>,
}

impl AppState {
    pub fn production() -> Self {
        Self {
            claude: Arc::new(crate::claude::ClaudeAdapter::new()),
            codex: Arc::new(crate::codex::CodexAdapter::new()),
        }
    }

    fn adapter(&self, id: AgentId) -> &Arc<dyn AgentAdapter> {
        match id {
            AgentId::Claude => &self.claude,
            AgentId::Codex => &self.codex,
        }
    }
}

#[tauri::command]
pub fn list_agents(state: tauri::State<AppState>) -> Vec<AgentInfo> {
    vec![state.claude.info(), state.codex.info()]
}

#[tauri::command]
pub fn list_plugins(agent_id: AgentId, state: tauri::State<AppState>) -> Result<AgentTabDto, AdapterError> {
    state.adapter(agent_id).list_tab()
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
pub fn refresh(agent_id: AgentId, state: tauri::State<AppState>) -> Result<AgentTabDto, AdapterError> {
    state.adapter(agent_id).list_tab()
}
