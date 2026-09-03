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
