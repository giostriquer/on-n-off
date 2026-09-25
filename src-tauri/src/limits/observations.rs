//! Provider-neutral merging for independently dated quota-window observations.

use chrono::{DateTime, SecondsFormat, Utc};

use crate::dto::{
    LimitWindowDto, LimitsCreditsDto, LimitsCreditsSpentDto, LimitsResetCreditsDto,
    LimitsWorkspaceCreditsDto, ProviderLimitsDto,
};

pub(super) struct ObservedWindowSet {
    /// When the windows were observed; `None` only for a set that carries figures and no windows.
    observed_at: Option<DateTime<Utc>>,
    plan: Option<String>,
    subscription_status: Option<String>,
    windows: Vec<LimitWindowDto>,
    credits: Option<LimitsCreditsDto>,
    workspace_credits: Option<LimitsWorkspaceCreditsDto>,
    credits_spent: Option<LimitsCreditsSpentDto>,
    reset_credits: Option<LimitsResetCreditsDto>,
}

impl ObservedWindowSet {
    /// A remembered account's observations. Windows need a date to merge by; figures alone (a credit
    /// balance, banked resets) are kept without one.
    pub(super) fn from_account(dto: ProviderLimitsDto) -> Option<Self> {
        let observed_at = dto
            .windows
            .iter()
            .filter_map(|window| DateTime::parse_from_rfc3339(&window.observed_at).ok())
            .max()
            .map(|at| at.with_timezone(&Utc));
        if !dto.has_observations() || (observed_at.is_none() && !dto.windows.is_empty()) {
            return None;
        }
        Some(Self {
            observed_at,
            plan: dto.plan,
            subscription_status: dto.subscription_status,
            windows: dto.windows,
            credits: dto.credits,
            workspace_credits: dto.workspace_credits,
            credits_spent: dto.credits_spent,
            reset_credits: dto.reset_credits,
        })
    }
}

/// Merge the remembered observations into a failed read, per quota window: the newer observation
/// of each window wins. Remembered plan, subscription-status and credit metadata fill what the read
/// did not report.
pub(super) fn merge_windows(
    mut current: ProviderLimitsDto,
    remembered: Option<ObservedWindowSet>,
) -> ProviderLimitsDto {
    if let Some(remembered) = &remembered {
        current.plan = current.plan.take().or_else(|| remembered.plan.clone());
        current.subscription_status = current
            .subscription_status
            .take()
            .or_else(|| remembered.subscription_status.clone());
        current.credits = current
            .credits
            .take()
            .or_else(|| remembered.credits.clone());
        current.workspace_credits = current
            .workspace_credits
            .take()
            .or_else(|| remembered.workspace_credits.clone());
        current.credits_spent = current
            .credits_spent
            .take()
            .or_else(|| remembered.credits_spent.clone());
        current.reset_credits = current
            .reset_credits
            .take()
            .or_else(|| remembered.reset_credits.clone());
    }
    if let Some(mut snapshot) = remembered {
        let observed_at = snapshot
            .observed_at
            .map(|at| at.to_rfc3339_opts(SecondsFormat::Millis, true));
        for mut incoming in snapshot.windows.drain(..) {
            if let (true, Some(observed_at)) = (incoming.observed_at.is_empty(), &observed_at) {
                incoming.observed_at.clone_from(observed_at);
            }
            if let Some(index) = current
                .windows
                .iter()
                .position(|window| window.id == incoming.id)
            {
                if is_newer(&incoming, &current.windows[index]) {
                    current.windows[index] = incoming;
                }
            } else {
                current.windows.push(incoming);
            }
        }
    }
    current
        .windows
        .sort_by_key(|window| super::pipeline::kind_rank(window.kind));
    current
}

fn is_newer(incoming: &LimitWindowDto, existing: &LimitWindowDto) -> bool {
    let incoming = DateTime::parse_from_rfc3339(&incoming.observed_at);
    let existing = DateTime::parse_from_rfc3339(&existing.observed_at);
    matches!((incoming, existing), (Ok(incoming), Ok(existing)) if incoming > existing)
}

#[cfg(test)]
mod tests;
