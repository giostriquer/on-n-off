use serde::Deserialize;

use crate::dto::{
    AgentId, LimitWindowDto, LimitWindowKind, LimitsAccountDto, LimitsCreditsDto,
    LimitsCreditsSpentDto, LimitsResetCreditsDto, LimitsStatus, LimitsWorkspaceCreditsDto,
    ProviderLimitsDto, ReadStanding, Reading,
};

const FIXTURE: &str = include_str!("fixtures/standing-cases.json");

#[derive(Deserialize)]
pub(crate) struct Fixture {
    pub(crate) checking: String,
    pub(crate) cases: Vec<Case>,
}

#[derive(Deserialize)]
pub(crate) struct Case {
    pub(crate) name: String,
    provider: AgentId,
    account: Account,
    status: LimitsStatus,
    observes: Observes,
    message: Option<String>,
    pub(crate) standing: serde_json::Value,
    pub(crate) notch: serde_json::Value,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
enum Account {
    Current,
    Saved,
    Remembered,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
enum Observes {
    Windows,
    Credits,
    WorkspaceShare,
    CreditsSpent,
    BankedResets,
    NoBankedResets,
    Nothing,
}

pub(crate) fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("the read standing cases")
}

impl Case {
    pub(crate) fn card(&self) -> ProviderLimitsDto {
        ProviderLimitsDto {
            provider: self.provider,
            status: self.status,
            message: self.message.clone(),
            account: Some(LimitsAccountDto {
                id: "acct".into(),
                legacy_id: None,
                label: Some("you@example.com".into()),
            }),
            current_account: matches!(self.account, Account::Current),
            saved_profile: matches!(self.account, Account::Saved),
            archived: false,
            reading: reading(self.observes),
        }
    }

    pub(crate) fn expected(&self) -> ReadStanding {
        serde_json::from_value(self.standing.clone()).expect("a read standing")
    }
}

fn reading(observes: Observes) -> Reading {
    match observes {
        Observes::Windows => Reading {
            windows: vec![LimitWindowDto {
                id: "weekly".into(),
                label: "Weekly · all models".into(),
                kind: LimitWindowKind::Weekly,
                used_percent: 41.0,
                resets_at: Some("2026-09-05T08:00:00Z".into()),
                window_seconds: None,
                observed_at: "2026-09-01T10:00:00Z".into(),
                role: None,
                starts_on_first_message: false,
            }],
            ..Reading::default()
        },
        Observes::Credits => Reading {
            credits: Some(LimitsCreditsDto {
                balance: "12.50".into(),
                unlimited: false,
            }),
            ..Reading::default()
        },
        Observes::WorkspaceShare => Reading {
            workspace_credits: Some(LimitsWorkspaceCreditsDto {
                limit: "25000".into(),
                used: "8000".into(),
                used_percent: 32.0,
                resets_at: None,
                reached: false,
            }),
            ..Reading::default()
        },
        Observes::CreditsSpent => Reading {
            credits_spent: Some(LimitsCreditsSpentDto {
                last_7_days: 12.0,
                last_30_days: 40.0,
                updated_at: None,
            }),
            ..Reading::default()
        },
        Observes::BankedResets => banked(1),
        Observes::NoBankedResets => banked(0),
        Observes::Nothing => Reading::default(),
    }
}

fn banked(available_count: u32) -> Reading {
    Reading {
        reset_credits: Some(LimitsResetCreditsDto {
            available_count,
            next_expires_at: None,
            resets: Vec::new(),
        }),
        ..Reading::default()
    }
}
