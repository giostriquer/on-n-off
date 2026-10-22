use super::super::supersedes;
use crate::dto::{
    AgentId, LimitWindowKind, LimitsAccountDto, LimitsCreditsDto, LimitsCreditsSpentDto,
    LimitsResetCreditsDto, LimitsStatus, LimitsWorkspaceCreditsDto, ProviderLimitsDto, Reading,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Side {
    provider: AgentId,
    status: LimitsStatus,
    current: bool,
    id: String,
    legacy_id: Option<String>,
    label: Option<String>,
    observed: Observed,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Observed {
    Windows,
    Credits,
    WorkspaceCredits,
    CreditsSpent,
    BankedResets,
    NoBankedResets,
    Nothing,
}

fn banked(available_count: u32) -> Option<LimitsResetCreditsDto> {
    Some(LimitsResetCreditsDto {
        available_count,
        next_expires_at: None,
        resets: Vec::new(),
    })
}

fn reading(observed: &Observed) -> Reading {
    match observed {
        Observed::Windows => Reading {
            windows: vec![crate::limits::window::fixture(
                "weekly",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                40.0,
                None,
            )],
            ..Reading::default()
        },
        Observed::Credits => Reading {
            credits: Some(LimitsCreditsDto {
                balance: "5".into(),
                unlimited: false,
            }),
            ..Reading::default()
        },
        Observed::WorkspaceCredits => Reading {
            workspace_credits: Some(LimitsWorkspaceCreditsDto {
                limit: "100".into(),
                used: "40".into(),
                used_percent: 40.0,
                resets_at: None,
                reached: false,
            }),
            ..Reading::default()
        },
        Observed::CreditsSpent => Reading {
            credits_spent: Some(LimitsCreditsSpentDto {
                last_7_days: 1.0,
                last_30_days: 2.0,
                updated_at: None,
            }),
            ..Reading::default()
        },
        Observed::BankedResets => Reading {
            reset_credits: banked(1),
            ..Reading::default()
        },
        Observed::NoBankedResets => Reading {
            reset_credits: banked(0),
            ..Reading::default()
        },
        Observed::Nothing => Reading::default(),
    }
}

#[derive(Deserialize)]
struct Case {
    case: String,
    scoped: Side,
    legacy: Side,
    supersedes: bool,
}

fn card(side: Side) -> ProviderLimitsDto {
    ProviderLimitsDto {
        reading: reading(&side.observed),
        status: side.status,
        current_account: side.current,
        account: Some(LimitsAccountDto {
            id: side.id,
            legacy_id: side.legacy_id,
            label: side.label,
        }),
        ..ProviderLimitsDto::for_test(side.provider, "unused")
    }
}

#[test]
fn every_shared_case_folds_as_the_dev_mock_folds_it() {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("../../column/fixtures/supersedes-cases.json")).unwrap();
    assert!(cases.iter().any(|case| case.supersedes));
    assert!(cases.iter().any(|case| !case.supersedes));
    for case in cases {
        assert_eq!(
            supersedes(&card(case.scoped), &card(case.legacy)),
            case.supersedes,
            "{}",
            case.case
        );
    }
}
