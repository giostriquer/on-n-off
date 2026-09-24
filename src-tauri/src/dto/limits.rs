//! Subscription limits: what the Limits screen, the tray and the side notch show for each
//! provider account, and what the remembered per-account snapshots store.

use serde::{Deserialize, Serialize};

use super::AgentId;

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

/// A business workspace member's share of the workspace's pooled credits: Codex's spend control,
/// which caps how many of the workspace's credits this member may use until it resets. A member's
/// own credit balance (`LimitsCreditsDto`) is usually 0 in a workspace, because the credits are
/// the workspace's.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsWorkspaceCreditsDto {
    /// Credits this member may use, as the provider states the amount: a finite number of at least
    /// zero, which may carry decimals.
    pub limit: String,
    /// Credits this member has used of it.
    pub used: String,
    /// How much of the share is used, 0–100, worked out once by the reader (`limits/codex.rs`).
    /// Required: no released version stored a share without it.
    pub used_percent: f64,
    /// RFC 3339 instant the share resets, when the provider says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    /// The provider says this member has used the whole share.
    #[serde(default)]
    pub reached: bool,
}

/// Credits a business workspace member has spent lately, counted the way the Codex app's "Credit
/// usage history" counts them: each UTC day's per-model credits, summed over the last 7 and the last
/// 30 days up to today. There is no limit to measure it against; it is only reported.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsCreditsSpentDto {
    pub last_7_days: f64,
    pub last_30_days: f64,
    /// RFC 3339 instant the provider's usage data runs up to; it can trail the read by hours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Banked rate-limit resets: one-time resets saved to the account until used or expired. Codex's
/// can be spent from on-n-off; Claude's are only reported, and spent with Claude Code's `/limit-reset`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsResetCreditsDto {
    pub available_count: u32,
    /// RFC 3339 instant when the soonest-expiring available reset lapses, when the provider says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_expires_at: Option<String>,
}

/// A paid reset Codex is offering this account right now, read from the backend-owned banner on a
/// usage read. It is an offer in flight, not a standing entitlement: it appears only once a limit
/// is reached, so its absence never means the account could not buy one. on-n-off shows it and
/// never sells it; the purchase lives on the provider's own site.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsResetOfferDto {
    /// What it costs, when the provider named a price. An offer without one is still an offer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<LimitsPriceDto>,
}

/// A price the way the provider states it: minor units and the currency they belong to. Both or
/// neither, so an amount can never be shown without knowing what it counts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsPriceDto {
    /// Cents, or whatever the currency's minor unit is. The UI divides by that currency's exponent.
    pub amount_minor_units: u64,
    /// An ISO 4217 code, upper-cased: exactly three letters, or the price is not read at all.
    pub currency: String,
}

/// What Codex did with a request to spend one banked reset, in Codex's own wire names. `Unknown`
/// catches an outcome this build does not recognise: the request still went through, so it is not
/// reported as a failure.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ResetCreditOutcome {
    Reset,
    NothingToReset,
    NoCredit,
    AlreadyRedeemed,
    #[serde(other)]
    Unknown,
}

/// Which subscription account a limits snapshot belongs to. `id` is the provider's stable account
/// id (or `default` when the CLI stores none); `label` is the human name (email) when known.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsAccountDto {
    pub id: String,
    /// Previous unscoped cache key; used only to suppress superseded history, never transfer usage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_id: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_credits: Option<LimitsWorkspaceCreditsDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits_spent: Option<LimitsCreditsSpentDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_credits: Option<LimitsResetCreditsDto>,
    /// Never remembered: an offer withdrawn between reads must disappear with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_offer: Option<LimitsResetOfferDto>,
}

impl ProviderLimitsDto {
    /// Whether this read observed anything about the account worth keeping: quota windows or any
    /// of the figures beside them. One definition for every place that decides that.
    pub fn has_observations(&self) -> bool {
        !self.windows.is_empty() || self.has_figures()
    }

    /// The figures a read reports beside its windows: a credit balance, a workspace-credit share,
    /// the credits spent lately or banked resets. One list, so a figure added later is counted
    /// everywhere at once.
    pub fn has_figures(&self) -> bool {
        self.credits.is_some()
            || self.workspace_credits.is_some()
            || self.credits_spent.is_some()
            || self.has_banked_resets()
    }

    /// Every current Codex read and most Claude reads report a reset count, usually 0, so only a
    /// positive count is an observation; the 0 still matters when it replaces a remembered count.
    pub fn has_banked_resets(&self) -> bool {
        self.reset_credits
            .as_ref()
            .is_some_and(|resets| resets.available_count > 0)
    }

    /// A successful read that could not tell how many resets are banked keeps the count `previous`
    /// knew; one that answered, 0 included, replaces it.
    pub fn keep_reset_credits_from(&mut self, previous: &Self) {
        if self.reset_credits.is_none() {
            self.reset_credits.clone_from(&previous.reset_credits);
        }
    }
}
