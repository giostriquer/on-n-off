use super::super::supersedes;
use crate::dto::{
    AgentId, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto, Reading,
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
    observed: bool,
}

#[derive(Deserialize)]
struct Case {
    case: String,
    scoped: Side,
    legacy: Side,
    supersedes: bool,
}

fn card(side: Side) -> ProviderLimitsDto {
    let windows = if side.observed {
        vec![crate::limits::window::fixture(
            "weekly",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            40.0,
            None,
        )]
    } else {
        Vec::new()
    };
    ProviderLimitsDto {
        reading: Reading {
            windows,
            ..Reading::default()
        },
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
