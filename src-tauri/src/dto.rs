use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentId {
    Claude,
    Codex,
}

impl AgentId {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    CliMissing,
    CliTooOld,
    Parse,
    Write,
    Message,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterError {
    pub kind: ErrorKind,
    pub message: String,
    pub path: Option<String>,
}

impl AdapterError {
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Message,
            message: message.into(),
            path: None,
        }
    }

    pub fn write(message: impl Into<String>, path: Option<String>) -> Self {
        Self {
            kind: ErrorKind::Write,
            message: message.into(),
            path,
        }
    }

    pub fn parse(message: impl Into<String>, path: Option<String>) -> Self {
        Self {
            kind: ErrorKind::Parse,
            message: message.into(),
            path,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: AgentId,
    pub display_name: String,
    pub cli_ok: bool,
    pub cli_error: Option<String>,
    pub install_git: bool,
    pub install_folder: bool,
    pub plugin_toggle: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillDto {
    pub id: String,
    pub plugin_id: Option<String>,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub togglable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginDto {
    pub id: String,
    pub name: String,
    pub source: String,
    pub enabled: bool,
    pub skills: Vec<SkillDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTabDto {
    pub plugins: Vec<PluginDto>,
    pub user_skills: Vec<SkillDto>,
}

impl AgentTabDto {
    pub fn skill(&self, id: &str) -> Option<&SkillDto> {
        self.plugins
            .iter()
            .flat_map(|plugin| plugin.skills.iter())
            .chain(self.user_skills.iter())
            .find(|skill| skill.id == id)
    }

    pub fn ensure_togglable(&self, skill_id: &str) -> Result<(), AdapterError> {
        match self.skill(skill_id) {
            None => Err(AdapterError::message(format!("skill not found: {skill_id}"))),
            Some(skill) if !skill.togglable => {
                Err(AdapterError::message(format!("skill is not togglable: {skill_id}")))
            }
            Some(_) => Ok(()),
        }
    }

    pub fn ensure_plugin(&self, plugin_id: &str) -> Result<&PluginDto, AdapterError> {
        self.plugins
            .iter()
            .find(|plugin| plugin.id == plugin_id)
            .ok_or_else(|| AdapterError::message(format!("plugin not found: {plugin_id}")))
    }
}
