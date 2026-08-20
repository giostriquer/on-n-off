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
mod tests {
    use super::*;
    use crate::dto::{AgentId, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto};
    use crate::limits::json::window;

    fn observed(
        id: &str,
        label: &str,
        kind: LimitWindowKind,
        used_percent: f64,
        resets_at: Option<&str>,
        observed_at: &str,
    ) -> LimitWindowDto {
        LimitWindowDto {
            observed_at: observed_at.to_string(),
            ..window(id, label, kind, used_percent, resets_at.map(str::to_string))
        }
    }

    #[test]
    fn newer_windows_merge_independently_and_do_not_inherit_an_old_reset() {
        let current = ProviderLimitsDto {
            provider: AgentId::Claude,
            status: LimitsStatus::Unauthenticated,
            message: Some("Refresh paused".to_string()),
            account: Some(LimitsAccountDto {
                id: "uuid-1".to_string(),
                label: Some("me@example.com".to_string()),
            }),
            current_account: true,
            plan: None,
            windows: Vec::new(),
            credits: None,
        };
        let remembered = ObservedWindowSet::from_account(ProviderLimitsDto {
            provider: AgentId::Claude,
            status: LimitsStatus::Ok,
            message: None,
            account: current.account.clone(),
            current_account: false,
            plan: Some("max".to_string()),
            windows: vec![
                observed(
                    "weekly_all",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    39.0,
                    Some("2026-08-24T12:00:00Z"),
                    "2026-08-17T15:00:00Z",
                ),
                observed(
                    "weekly_opus",
                    "Weekly · Opus",
                    LimitWindowKind::Model,
                    91.0,
                    Some("2026-08-24T12:00:00Z"),
                    "2026-08-17T15:00:00Z",
                ),
            ],
            credits: None,
        });
        let local = ObservedWindowSet::local(
            DateTime::parse_from_rfc3339("2026-08-18T03:07:53Z")
                .unwrap()
                .with_timezone(&Utc),
            vec![
                window(
                    "weekly_all",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    63.0,
                    None,
                ),
                window(
                    "session",
                    "5 hour · all models",
                    LimitWindowKind::Session,
                    17.0,
                    None,
                ),
            ],
        );

        let merged = merge_windows(current, Some(local), remembered);
        let summary: Vec<(&str, f64, Option<&str>, &str)> = merged
            .windows
            .iter()
            .map(|window| {
                (
                    window.id.as_str(),
                    window.used_percent,
                    window.resets_at.as_deref(),
                    window.observed_at.as_str(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("weekly_all", 63.0, None, "2026-08-18T03:07:53.000Z"),
                ("session", 17.0, None, "2026-08-18T03:07:53.000Z"),
                (
                    "weekly_opus",
                    91.0,
                    Some("2026-08-24T12:00:00Z"),
                    "2026-08-17T15:00:00Z"
                ),
            ]
        );
        assert_eq!(merged.plan.as_deref(), Some("max"));
    }
}
