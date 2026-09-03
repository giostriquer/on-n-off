//! Provider-neutral merging for independently dated quota-window observations.

use chrono::{DateTime, SecondsFormat, Utc};

use crate::dto::{LimitWindowDto, LimitsCreditsDto, ProviderLimitsDto};

pub(super) struct ObservedWindowSet {
    observed_at: DateTime<Utc>,
    plan: Option<String>,
    windows: Vec<LimitWindowDto>,
    credits: Option<LimitsCreditsDto>,
}

impl ObservedWindowSet {
    pub(super) fn local(observed_at: DateTime<Utc>, windows: Vec<LimitWindowDto>) -> Self {
        Self {
            observed_at,
            plan: None,
            windows,
            credits: None,
        }
    }

    pub(super) fn from_account(dto: ProviderLimitsDto) -> Option<Self> {
        let observed_at = dto
            .windows
            .iter()
            .filter_map(|window| DateTime::parse_from_rfc3339(&window.observed_at).ok())
            .max()?
            .with_timezone(&Utc);
        Some(Self {
            observed_at,
            plan: dto.plan,
            windows: dto.windows,
            credits: dto.credits,
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
    current.credits = current.credits.or(remembered_credits);
    for mut snapshot in [remembered, local].into_iter().flatten() {
        let observed_at = snapshot
            .observed_at
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        for mut incoming in snapshot.windows.drain(..) {
            if incoming.observed_at.is_empty() {
                incoming.observed_at.clone_from(&observed_at);
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
