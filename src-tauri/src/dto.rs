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

    /// Lowercase id used for directory and file names (`claude`, `codex`, ...); matches the
    /// serde form.
    pub fn key(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::Cursor => "cursor",
        }
    }

    /// The CLI executable the user types (and that settings/diagnostics resolve). Cursor's is
    /// `agent`; the legacy `cursor-agent` alias is still accepted as an override.
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

/// Why the GitHub screen has no fresh data. Every value except `Ok` comes with a `hint` telling
/// the user what to do; `stale` says whether the last snapshot is being shown meanwhile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GithubStatus {
    Ok,
    /// The `gh` CLI is not on the search path.
    GhMissing,
    /// `gh` holds no github.com login (or could not answer).
    GhNotLoggedIn,
    /// GitHub rejected the token `gh` handed over, twice.
    TokenRejected,
    /// GitHub's rate limit is exhausted; polling pauses until it resets.
    RateLimited,
    /// DNS, TLS, timeout, an unexpected status, or an unreadable reply.
    Network,
}

/// The head commit's status-check rollup, collapsed to what a list row can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CiState {
    /// No checks reported (or a state this version does not know).
    None,
    /// `PENDING` or `EXPECTED`.
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

/// GitHub's `reviewDecision`, on the wire in GitHub's own spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

/// GitHub's `mergeable`: whether the head can be merged into the base without conflicts. It
/// rides beside `MergeState` because the two diverge on drafts: a draft with conflicts reports
/// `mergeStateStatus: DRAFT`, and only this field says `CONFLICTING`. `Unknown` also covers a
/// value GitHub has not computed yet (it computes on demand, so the next poll usually knows) and
/// one this version does not recognise.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mergeability {
    Mergeable,
    Conflicting,
    #[default]
    Unknown,
}

/// GitHub's `mergeStateStatus`, collapsed to what a list row can act on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeState {
    /// Every requirement is met (`CLEAN`, or `HAS_HOOKS`: clean with pre-receive hooks).
    Clean,
    /// Mergeable, but a non-required check is not passing.
    Unstable,
    /// Branch protection stops the merge: a missing review, a failing required check, and so on.
    Blocked,
    /// The head is behind the base and the base requires up-to-date branches.
    Behind,
    /// Merge conflicts.
    Dirty,
    Draft,
    /// Not computed yet, or a state this version does not know.
    #[default]
    Unknown,
}

/// The pull request's place in its repository's merge queue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubMergeQueueDto {
    /// 1-based position when GitHub reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubPrDto {
    /// GitHub's node id, stable across renames and pushes.
    pub id: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    /// `owner/name`.
    pub repo: String,
    pub author: String,
    pub is_draft: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_decision: Option<ReviewDecision>,
    pub ci: CiState,
    pub head_ref: String,
    pub base_ref: String,
    /// RFC 3339.
    pub updated_at: String,
    /// Only on the review-requested list: whether the request named the user or one of their teams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_request: Option<ReviewRequestKind>,
    /// Defaults keep a snapshot written before these fields existed loadable.
    #[serde(default)]
    pub mergeable: Mergeability,
    #[serde(default)]
    pub merge_state: MergeState,
    /// Present while the pull request sits in a merge queue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_queue: Option<GithubMergeQueueDto>,
    /// Auto-merge is enabled: GitHub merges once the requirements are met.
    #[serde(default)]
    pub auto_merge: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubPrListDto {
    /// Matches on GitHub, which can exceed `items.len()` (the query reads one page).
    pub total: u64,
    pub items: Vec<GithubPrDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubRateLimitDto {
    pub remaining: u64,
    /// RFC 3339.
    pub reset_at: String,
}

/// What one successful read produced: the part that is worth remembering on disk and showing
/// again while a later read fails.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubPrsData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer: Option<String>,
    /// RFC 3339 instant of the read that produced the lists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<String>,
    /// The scope qualifiers applied to `mine`.
    pub scope: Vec<String>,
    pub mine: GithubPrListDto,
    pub review_requested: GithubPrListDto,
    pub assigned: GithubPrListDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<GithubRateLimitDto>,
}

/// The GitHub screen's answer: an envelope (`status`, `hint`, `stale`, `warnings`) around the
/// data. Provider-side problems are a `status` + `hint`, never an error; `stale: true` means the
/// data comes from an earlier successful read.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubPrsDto {
    pub status: GithubStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub stale: bool,
    #[serde(flatten)]
    pub data: GithubPrsData,
    /// GraphQL `errors[]` messages that came with usable data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LimitsStatus {
    Ok,
    SignedOut,
    Unauthenticated,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LimitWindowKind {
    Session,
    Weekly,
    Model,
}

/// One rolling rate-limit window as reported by a provider's subscription endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindowDto {
    pub id: String,
    pub label: String,
    pub kind: LimitWindowKind,
    /// 0..=100.
    pub used_percent: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    /// Provider window duration used to correlate source-neutral observations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_seconds: Option<u64>,
    /// RFC 3339 instant when this individual window was observed.
    pub observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsCreditsDto {
    pub balance: String,
    pub unlimited: bool,
}

/// Which subscription account a limits snapshot belongs to. `id` is the provider's stable account
/// id (or `default` when the CLI stores none); `label` is the human name (email) when known.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsAccountDto {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Subscription rate-limit snapshot for one provider account. Provider-side problems are encoded
/// in `status` + `message` rather than returned as errors so the UI can render each provider
/// independently. `current_account: false` marks a remembered account the provider is no longer
/// signed into. Each window carries its own observation time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLimitsDto {
    pub provider: AgentId,
    pub status: LimitsStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<LimitsAccountDto>,
    pub current_account: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub windows: Vec<LimitWindowDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits: Option<LimitsCreditsDto>,
}

// ---------------------------------------------------------------------------
// Local items: skills and subagents copied out of a marketplace by on-n-off itself and
// tracked in `~/.on-n-off/installed-items.json` (see `item_install`).

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

/// How sure the dependency scanner is that one entry needs another (see `item_install::deps`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DepConfidence {
    Medium,
    High,
}

/// Another marketplace entry that this one refers to in its text.
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
    /// Path inside the marketplace repository, `/`-separated (skill folder or agent file).
    pub path: String,
    /// Sibling entries this one names in its text, best confidence first per target.
    #[serde(default)]
    pub depends_on: Vec<ItemDependencyDto>,
    /// Paths the text refers to that a local copy of the item will not contain.
    #[serde(default)]
    pub external_refs: Vec<String>,
    /// The text mentions `CLAUDE_PLUGIN_ROOT`, so it expects to run inside the plugin.
    #[serde(default)]
    pub uses_plugin_root: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplacePluginDto {
    pub name: String,
    pub version: Option<String>,
    pub description: String,
    /// `false` when the plugin source is something on-n-off cannot fetch (a bare URL, npm…).
    pub supported: bool,
    /// Set when the plugin lives in another GitHub repository than the marketplace itself.
    pub source: Option<ItemSourceDto>,
    pub skills: Vec<MarketplaceEntryDto>,
    pub agents: Vec<MarketplaceEntryDto>,
    /// Plugin-level assets a local copy never gets: `commands`, `hooks`, `mcp`.
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
    /// Overrides the request source for plugins hosted in another repository.
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
    /// The pick this outcome answers, so the UI can map conflicts back without guessing names.
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
    /// Where the item was copied from, so the UI can say so and link to it.
    pub source: ItemSourceDto,
    pub plugin_name: String,
    /// Skill folder or agent file inside the repository, `/`-separated.
    pub upstream_path: String,
    /// GitHub page of the item at the installed commit.
    pub upstream_url: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UpdateItemMode {
    Overwrite,
    Dismiss,
}
