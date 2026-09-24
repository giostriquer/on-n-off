//! Provider-neutral merging for independently dated quota-window observations.

use chrono::{DateTime, SecondsFormat, Utc};

use crate::dto::{
    LimitWindowDto, LimitsCreditsDto, LimitsResetCreditsDto, LimitsWorkspaceCreditsDto,
    ProviderLimitsDto,
};

pub(super) struct ObservedWindowSet {
    /// When the windows were observed; `None` only for a set that carries figures and no windows.
    observed_at: Option<DateTime<Utc>>,
    plan: Option<String>,
    windows: Vec<LimitWindowDto>,
    credits: Option<LimitsCreditsDto>,
    workspace_credits: Option<LimitsWorkspaceCreditsDto>,
    reset_credits: Option<LimitsResetCreditsDto>,
}

impl ObservedWindowSet {
    pub(super) fn local(observed_at: DateTime<Utc>, windows: Vec<LimitWindowDto>) -> Self {
        Self {
            observed_at: Some(observed_at),
            plan: None,
            windows,
            credits: None,
            workspace_credits: None,
            reset_credits: None,
        }
    }

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
            windows: dto.windows,
            credits: dto.credits,
            workspace_credits: dto.workspace_credits,
            reset_credits: dto.reset_credits,
        })
    }
}

/// Merge every observation per quota window. Remembered plan/credit metadata remains useful when a
/// newer local observation contains percentage windows only.
pub(super) fn merge_windows(
    mut current: ProviderLimitsDto,
    local: Option<ObservedWindowSet>,
    remembered: Option<ObservedWindowSet>,
) -> ProviderLimitsDto {
    let remembered_plan = remembered
        .as_ref()
        .and_then(|snapshot| snapshot.plan.clone());
    let remembered_credits = remembered
        .as_ref()
        .and_then(|snapshot| snapshot.credits.clone());
    current.plan = current.plan.or(remembered_plan);
    let remembered_reset_credits = remembered
        .as_ref()
        .and_then(|snapshot| snapshot.reset_credits.clone());
    let remembered_workspace_credits = remembered
        .as_ref()
        .and_then(|snapshot| snapshot.workspace_credits.clone());
    current.credits = current.credits.or(remembered_credits);
    current.workspace_credits = current.workspace_credits.or(remembered_workspace_credits);
    current.reset_credits = current.reset_credits.or(remembered_reset_credits);
    for mut snapshot in [remembered, local].into_iter().flatten() {
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
