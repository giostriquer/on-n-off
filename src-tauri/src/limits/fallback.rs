//! Selection and application of dated numbers used after a live Limits read fails.

use chrono::{DateTime, SecondsFormat, Utc};

use crate::dto::{LimitWindowDto, LimitsCreditsDto, ProviderLimitsDto};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    ClaudeDesktop,
    Remembered,
}

pub(super) struct NumberSnapshot {
    origin: Origin,
    observed_at: DateTime<Utc>,
    plan: Option<String>,
    windows: Vec<LimitWindowDto>,
    credits: Option<LimitsCreditsDto>,
}

impl NumberSnapshot {
    pub(super) fn claude_desktop(observed_at: DateTime<Utc>, windows: Vec<LimitWindowDto>) -> Self {
        Self {
            origin: Origin::ClaudeDesktop,
            observed_at,
            plan: None,
            windows,
            credits: None,
        }
    }

    pub(super) fn remembered(dto: ProviderLimitsDto) -> Option<Self> {
        let observed_at = DateTime::parse_from_rfc3339(&dto.fetched_at)
            .ok()?
            .with_timezone(&Utc);
        Some(Self {
            origin: Origin::Remembered,
            observed_at,
            plan: dto.plan,
            windows: dto.windows,
            credits: dto.credits,
        })
    }
}

/// Apply the newer fallback by instant. Remembered plan/credit metadata remains useful when the
/// newer Claude Desktop sample contains percentage windows only.
pub(super) fn apply_latest(
    mut current: ProviderLimitsDto,
    desktop: Option<NumberSnapshot>,
    remembered: Option<NumberSnapshot>,
) -> ProviderLimitsDto {
    let remembered_plan = remembered
        .as_ref()
        .and_then(|snapshot| snapshot.plan.clone());
    let remembered_credits = remembered
        .as_ref()
        .and_then(|snapshot| snapshot.credits.clone());
    let Some(mut selected) = [desktop, remembered]
        .into_iter()
        .flatten()
        .max_by_key(|snapshot| snapshot.observed_at)
    else {
        return current;
    };

    selected.plan = selected.plan.or(remembered_plan).or(current.plan);
    selected.credits = selected.credits.or(remembered_credits).or(current.credits);
    current.plan = selected.plan;
    current.windows = selected.windows;
    current.credits = selected.credits;
    current.fetched_at = selected
        .observed_at
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    if selected.origin == Origin::ClaudeDesktop {
        let status = current.message.take().unwrap_or_default();
        current.message = Some(format!("{status} Showing Claude Desktop usage."));
    }
    current
}
