use std::path::Path;

use crate::dto::{AdapterError, AgentInfo, AgentTabDto, ProjectDto};

pub trait AgentAdapter: Send + Sync {
    fn info(&self) -> AgentInfo;
    fn list_tab(&self) -> Result<AgentTabDto, AdapterError>;
    fn list_projects(&self) -> Vec<ProjectDto> {
        Vec::new()
    }
    fn list_scope(&self, project: Option<&str>) -> Result<AgentTabDto, AdapterError> {
        let mut tab = self.list_tab()?;
        if let Some(path) = project.map(str::trim).filter(|value| !value.is_empty()) {
            crate::project::overlay_project(&mut tab, Path::new(path), self.info().id);
        }
        Ok(tab)
    }

    fn set_plugin_enabled(&self, plugin_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
        let _ = (plugin_id, enabled);
        Err(AdapterError::message("plugin enable is not implemented yet"))
    }

    fn set_skill_enabled(&self, skill_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
        let _ = (skill_id, enabled);
        Err(AdapterError::message("skill toggle is not implemented yet"))
    }

    fn set_mcp_enabled(&self, mcp_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
        let _ = (mcp_id, enabled);
        Err(AdapterError::message("mcp toggle is not implemented yet"))
    }

    fn install_plugin(&self, source: &str) -> Result<AgentTabDto, AdapterError> {
        let _ = source;
        Err(AdapterError::message("install is not implemented yet"))
    }

    fn uninstall_plugin(&self, plugin_id: &str) -> Result<AgentTabDto, AdapterError> {
        let _ = plugin_id;
        Err(AdapterError::message("uninstall is not implemented yet"))
    }

    fn update_plugin(&self, plugin_id: &str) -> Result<AgentTabDto, AdapterError> {
        let _ = plugin_id;
        Err(AdapterError::message("update is not implemented yet"))
    }
}
