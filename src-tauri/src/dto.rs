use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentId {
    Claude,
    Codex,
    Antigravity,
    Cursor,
}

impl AgentId {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Antigravity => "Antigravity",
            Self::Cursor => "Cursor",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::Cursor => "cursor",
        }
    }

    pub fn binary_name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "agy",
            Self::Cursor => "agent",
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
    #[serde(default)]
    pub reads_hooks: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HookDto {
    pub id: String,
    pub event: String,
    #[serde(default)]
    pub matcher: String,
    pub handler: String,
    #[serde(default)]
    pub command: String,
    pub source: String,
    #[serde(default)]
    pub plugin_id: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTabDto {
    pub plugins: Vec<PluginDto>,
    pub user_skills: Vec<SkillDto>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerDto>,
    #[serde(default)]
    pub hooks: Vec<HookDto>,
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
    #[serde(default)]
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsageHistoryState {
    Empty,
    Kept,
    Unreadable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageHistoryStatusDto {
    pub state: UsageHistoryState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_since: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folded_through: Option<String>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GithubStatus {
    Ok,
    GhMissing,
    GhNotLoggedIn,
    TokenRejected,
    RateLimited,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CiState {
    None,
    Pending,
    Success,
    Failure,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewRequestKind {
    Direct,
    Team,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mergeability {
    Mergeable,
    Conflicting,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeState {
    Clean,
    Unstable,
    Blocked,
    Behind,
    Dirty,
    Draft,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeKind {
    Conflicts,
    Queued,
    AutoMerge,
    Ready,
    Behind,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubMergeQueueDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubPrDto {
    pub id: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub repo: String,
    pub author: String,
    pub is_draft: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_decision: Option<ReviewDecision>,
    pub ci: CiState,
    pub head_ref: String,
    pub base_ref: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_request: Option<ReviewRequestKind>,
    #[serde(default)]
    pub mergeable: Mergeability,
    #[serde(default)]
    pub merge_state: MergeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_queue: Option<GithubMergeQueueDto>,
    #[serde(default)]
    pub auto_merge: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_kind: Option<MergeKind>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubPrListDto {
    pub total: u64,
    pub items: Vec<GithubPrDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubRateLimitDto {
    pub remaining: u64,
    pub reset_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubPrsData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<String>,
    pub scope: Vec<String>,
    pub mine: GithubPrListDto,
    pub review_requested: GithubPrListDto,
    pub assigned: GithubPrListDto,
    #[serde(default)]
    pub merged: GithubPrListDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<GithubRateLimitDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubPrsDto {
    pub status: GithubStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub stale: bool,
    #[serde(flatten)]
    pub data: GithubPrsData,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

mod limits;
pub use limits::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Skill,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ItemScope {
    Global,
    #[serde(rename_all = "camelCase")]
    Project {
        project_path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DepConfidence {
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemDependencyDto {
    pub plugin_name: String,
    pub kind: ItemKind,
    pub path: String,
    pub name: String,
    pub confidence: DepConfidence,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceEntryDto {
    pub name: String,
    pub description: String,
    pub path: String,
    #[serde(default)]
    pub depends_on: Vec<ItemDependencyDto>,
    #[serde(default)]
    pub external_refs: Vec<String>,
    #[serde(default)]
    pub uses_plugin_root: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplacePluginDto {
    pub name: String,
    pub version: Option<String>,
    pub description: String,
    pub supported: bool,
    pub source: Option<ItemSourceDto>,
    pub skills: Vec<MarketplaceEntryDto>,
    pub agents: Vec<MarketplaceEntryDto>,
    #[serde(default)]
    pub extras: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceInspectDto {
    pub is_marketplace: bool,
    pub commit_sha: String,
    pub marketplace_name: String,
    pub plugins: Vec<MarketplacePluginDto>,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct ItemSourceDto {
    pub owner: String,
    pub repo: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemPick {
    pub plugin_name: String,
    pub kind: ItemKind,
    pub path: String,
    #[serde(default)]
    pub source: Option<ItemSourceDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemTarget {
    pub provider: AgentId,
    pub scope: ItemScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallItemsRequest {
    pub source: ItemSourceDto,
    pub commit_sha: String,
    pub items: Vec<ItemPick>,
    pub targets: Vec<ItemTarget>,
    #[serde(default)]
    pub overwrite_unmanaged: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ItemOutcomeStatus {
    Installed,
    Replaced,
    Skipped,
    Conflict,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemOutcomeDto {
    pub provider: AgentId,
    pub kind: ItemKind,
    pub name: String,
    pub plugin_name: String,
    pub path: String,
    pub target_path: String,
    pub status: ItemOutcomeStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallItemsResultDto {
    pub commit_sha: String,
    pub sha_moved: bool,
    pub outcomes: Vec<ItemOutcomeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ItemUpstream {
    Unknown,
    Current,
    #[serde(rename_all = "camelCase")]
    UpdateAvailable {
        commit_sha: String,
        plugin_version: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemStatusDto {
    pub id: String,
    pub provider: AgentId,
    pub kind: ItemKind,
    pub name: String,
    pub display_name: String,
    pub target_path: String,
    pub installed_version: Option<String>,
    pub installed_sha: String,
    pub modified: bool,
    pub missing: bool,
    pub upstream: ItemUpstream,
    pub source: ItemSourceDto,
    pub plugin_name: String,
    pub upstream_path: String,
    pub upstream_url: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UpdateItemMode {
    Overwrite,
    Dismiss,
}
