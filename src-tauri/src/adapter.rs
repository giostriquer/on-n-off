use crate::dto::{AdapterError, AgentInfo, AgentTabDto};

pub trait AgentAdapter: Send + Sync {
    fn info(&self) -> AgentInfo;
    fn list_tab(&self) -> Result<AgentTabDto, AdapterError>;

    fn set_plugin_enabled(&self, plugin_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
        let _ = (plugin_id, enabled);
        Err(AdapterError::message("plugin enable is not implemented yet"))
    }

    fn set_skill_enabled(&self, skill_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
        let _ = (skill_id, enabled);
        Err(AdapterError::message("skill toggle is not implemented yet"))
    }

    fn install_plugin(&self, source: &str) -> Result<AgentTabDto, AdapterError> {
        let _ = source;
        Err(AdapterError::message("install is not implemented yet"))
    }

    fn uninstall_plugin(&self, plugin_id: &str) -> Result<AgentTabDto, AdapterError> {
        let _ = plugin_id;
        Err(AdapterError::message("uninstall is not implemented yet"))
    }
}
