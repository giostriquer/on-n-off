use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentId {
    Claude,
    Codex,
    Antigravity,
}

impl AgentId {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Antigravity => "Antigravity",
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
    #[serde(default)]
    pub origin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginDto {
    pub id: String,
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub upstream: String,
    #[serde(default)]
    pub out_of_sync: bool,
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub togglable: bool,
    pub skills: Vec<SkillDto>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpServerDto {
    pub id: String,
    pub name: String,
    pub system: String,
    pub source: String,
    pub enabled: bool,
    pub togglable: bool,
    #[serde(default)]
    pub origin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTabDto {
    pub plugins: Vec<PluginDto>,
    pub user_skills: Vec<SkillDto>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDto {
    pub id: String,
    pub label: String,
    pub path: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub skill_count: u32,
    #[serde(default)]
    pub mcp_count: u32,
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
            None => Err(AdapterError::message(format!(
                "skill not found: {skill_id}"
            ))),
            Some(skill) if !skill.togglable => Err(AdapterError::message(format!(
                "skill is not togglable: {skill_id}"
            ))),
            Some(_) => Ok(()),
        }
    }

    pub fn ensure_plugin(&self, plugin_id: &str) -> Result<&PluginDto, AdapterError> {
        self.plugins
            .iter()
            .find(|plugin| plugin.id == plugin_id)
            .ok_or_else(|| AdapterError::message(format!("plugin not found: {plugin_id}")))
    }

    pub fn ensure_plugin_togglable(&self, plugin_id: &str) -> Result<&PluginDto, AdapterError> {
        let plugin = self.ensure_plugin(plugin_id)?;
        if !plugin.togglable {
            return Err(AdapterError::message(format!(
                "plugin is not togglable: {plugin_id}"
            )));
        }
        Ok(plugin)
    }

    pub fn ensure_mcp_togglable(&self, mcp_id: &str) -> Result<(), AdapterError> {
        match self.mcp_servers.iter().find(|server| server.id == mcp_id) {
            None => Err(AdapterError::message(format!(
                "mcp server not found: {mcp_id}"
            ))),
            Some(server) if !server.togglable => Err(AdapterError::message(format!(
                "mcp server is not togglable: {mcp_id}"
            ))),
            Some(_) => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummaryInput {
    pub since_day: String,
    pub until_day: String,
    pub time_zone: String,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub since_time: Option<String>,
    #[serde(default)]
    pub until_time: Option<String>,
    /// When true, bypass the aggregated summary cache and rescan.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageCostSource {
    ProviderReported,
    ModelPriced,
    Unpriced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsageSourceStatus {
    Ok,
    Missing,
    Partial,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsagePricingStatus {
    Fresh,
    Cached,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageTokenTotalsDto {
    pub uncached_input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_creation_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageBucketDto {
    pub day: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hour_start: Option<String>,
    pub provider: AgentId,
    pub model: String,
    pub totals: UsageTokenTotalsDto,
    pub cost_usd: f64,
    pub cache_savings_usd: f64,
    pub cost_source: UsageCostSource,
    pub records: u64,
    pub unpriced_records: u64,
    pub sessions: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSourceDto {
    pub provider: AgentId,
    pub status: UsageSourceStatus,
    pub scanned_files: u64,
    pub skipped_files: u64,
    pub malformed_records: u64,
    pub distinct_sessions: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub resolved_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsagePricingDto {
    pub status: UsagePricingStatus,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<String>,
    pub known_models: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummaryDto {
    pub read_at: String,
    pub time_zone: String,
    pub since_day: String,
    pub until_day: String,
    pub buckets: Vec<UsageBucketDto>,
    pub sources: Vec<UsageSourceDto>,
    pub pricing: UsagePricingDto,
    pub scan_duration_ms: u64,
    /// True when served from the aggregated summary cache (no transcript walk).
    #[serde(default)]
    pub cache_hit: bool,
}
