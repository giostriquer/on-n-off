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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindowDto {
    pub id: String,
    pub label: String,
    pub kind: LimitWindowKind,
    pub used_percent: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_seconds: Option<u64>,
    pub observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsCreditsDto {
    pub balance: String,
    pub unlimited: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsWorkspaceCreditsDto {
    pub limit: String,
    pub used: String,
    pub used_percent: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    #[serde(default)]
    pub reached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsCreditsSpentDto {
    pub last_7_days: f64,
    pub last_30_days: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsSubscriptionDto {
    pub active_until: String,
    pub will_renew: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<SubscriptionNote>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubscriptionNote {
    Cancelled,
    PlanChange,
    PastDue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsResetCreditsDto {
    pub available_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resets: Vec<LimitsBankedResetDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsBankedResetDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsResetOfferDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<LimitsPriceDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsPriceDto {
    pub amount_minor_units: u64,
    pub currency: String,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingResetSpendDto {
    pub account_id: String,
    pub due_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsAccountDto {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
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
    pub subscription: Option<LimitsSubscriptionDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_credits: Option<LimitsResetCreditsDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_offer: Option<LimitsResetOfferDto>,
}

impl Reading {
    pub fn has_observations(&self) -> bool {
        !self.windows.is_empty() || self.has_figures()
    }

    pub fn has_figures(&self) -> bool {
        self.credits.is_some()
            || self.workspace_credits.is_some()
            || self.credits_spent.is_some()
            || self.has_banked_resets()
    }

    pub fn has_banked_resets(&self) -> bool {
        self.reset_credits
            .as_ref()
            .is_some_and(|resets| resets.available_count > 0)
    }

    pub fn limit_left_percent(&self) -> Option<f64> {
        self.windows
            .iter()
            .filter(|window| {
                matches!(
                    window.kind,
                    LimitWindowKind::Weekly | LimitWindowKind::Session
                )
            })
            .map(|window| window.used_percent)
            .reduce(f64::max)
            .map(|used| (100.0 - used).max(0.0))
    }
}

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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub saved_profile: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
    #[serde(flatten)]
    pub reading: Reading,
}

#[cfg(test)]
impl ProviderLimitsDto {
    pub fn for_test(provider: AgentId, account_id: &str) -> Self {
        Self {
            provider,
            status: LimitsStatus::Ok,
            message: None,
            account: Some(LimitsAccountDto {
                id: account_id.to_string(),
                legacy_id: None,
                label: None,
            }),
            current_account: true,
            saved_profile: false,
            archived: false,
            reading: Reading::default(),
        }
    }

    pub fn labelled(mut self, label: &str) -> Self {
        if let Some(account) = &mut self.account {
            account.label = Some(label.to_string());
        }
        self
    }

    pub fn with_legacy_id(mut self, legacy_id: &str) -> Self {
        if let Some(account) = &mut self.account {
            account.legacy_id = Some(legacy_id.to_string());
        }
        self
    }

    pub fn with_reading(self, reading: Reading) -> Self {
        Self { reading, ..self }
    }
}
