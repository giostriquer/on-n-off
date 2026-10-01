use super::*;
use crate::dto::LimitWindowKind;
use crate::limits::json::window;
use serde_json::{json, Value};

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
fn an_answer_without_a_weekly_window_keeps_the_remembered_weekly() {
    let answered = Outcome::Answered {
        asked_what_was_spent: false,
        asked_about_renewal: false,
    };
    let own = Reading {
        windows: vec![observed(
            "session",
            "5 hour · all models",
            LimitWindowKind::Session,
            17.0,
            None,
            "2026-08-18T03:07:53.000Z",
        )],
        ..Reading::default()
    };
    let remembered = Reading {
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
                None,
                "2026-08-17T15:00:00Z",
            ),
        ],
        ..Reading::default()
    };

    let kept = own.clone().keeping(remembered.clone(), answered);
    let summary: Vec<(&str, f64, &str)> = kept
        .windows
        .iter()
        .map(|window| {
            (
                window.id.as_str(),
                window.used_percent,
                window.observed_at.as_str(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("weekly_all", 39.0, "2026-08-17T15:00:00Z"),
            ("session", 17.0, "2026-08-18T03:07:53.000Z"),
        ]
    );

    let mut with_weekly = own;
    with_weekly.windows.insert(
        0,
        observed(
            "weekly_all",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            63.0,
            None,
            "2026-08-18T03:07:53.000Z",
        ),
    );
    let kept = with_weekly.keeping(remembered, answered);
    let used: Vec<f64> = kept.windows.iter().map(|w| w.used_percent).collect();
    assert_eq!(used, [63.0, 17.0]);
}

#[test]
fn only_an_answer_with_other_windows_keeps_the_remembered_weekly() {
    let answered = Outcome::Answered {
        asked_what_was_spent: false,
        asked_about_renewal: false,
    };
    let remembered = Reading {
        windows: vec![observed(
            "weekly_all",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            39.0,
            None,
            "2026-08-17T15:00:00Z",
        )],
        ..Reading::default()
    };
    let session_only = Reading {
        windows: vec![observed(
            "session",
            "5 hour · all models",
            LimitWindowKind::Session,
            17.0,
            None,
            "2026-08-18T03:07:53.000Z",
        )],
        ..Reading::default()
    };
    let figures_only = Reading {
        credits: Some(crate::dto::LimitsCreditsDto {
            balance: "3".to_string(),
            unlimited: false,
        }),
        ..Reading::default()
    };

    let ids = |reading: Reading| -> Vec<String> {
        reading
            .windows
            .into_iter()
            .map(|window| window.id)
            .collect()
    };
    assert_eq!(
        ids(session_only.keeping(remembered.clone(), answered)),
        ["weekly_all", "session"]
    );
    assert!(ids(figures_only.keeping(remembered, answered)).is_empty());
}

#[test]
fn a_paused_refresh_keeps_banked_resets_remembered_without_any_windows() {
    let reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 2,
        next_expires_at: None,
        resets: Vec::new(),
    });
    let remembered = Reading {
        plan: Some("pro".to_string()),
        reset_credits: reset_credits.clone(),
        ..Reading::default()
    };

    let merged = Reading::default().keeping(remembered, Outcome::Failed);

    assert_eq!(merged.reset_credits, reset_credits);
    assert_eq!(merged.reset_offer, None);
    assert_eq!(merged.plan.as_deref(), Some("pro"));
    assert!(merged.windows.is_empty());
}

#[test]
fn a_count_lapses_at_its_expiry_itself() {
    let expires_at = "2026-09-22T12:00:00+00:00";
    let resets = crate::dto::LimitsResetCreditsDto {
        available_count: 2,
        next_expires_at: Some(expires_at.to_string()),
        resets: Vec::new(),
    };
    let at = parse_observed_at(expires_at).unwrap();
    let expiry = resets.next_expires_at.as_deref();
    assert!(passed(expiry, at));
    assert!(!passed(expiry, at - chrono::Duration::seconds(1)));
    assert!(!passed(None, at), "no known expiry never lapses");
}

fn every_field(tag: &str, used: f64) -> Value {
    json!({
        "plan": tag,
        "windows": [{"id": tag, "label": tag, "kind": "weekly", "usedPercent": used,
                     "observedAt": "2026-08-17T10:00:00.000Z"},
                    {"id": format!("{tag}-session"), "label": tag, "kind": "session",
                     "usedPercent": used, "observedAt": "2026-08-17T10:00:00.000Z"}],
        "credits": {"balance": tag, "unlimited": false},
        "workspaceCredits": {"limit": "25000", "used": tag, "usedPercent": used, "reached": false},
        "creditsSpent": {"last7Days": used, "last30Days": used},
        "subscription": {"activeUntil": tag, "willRenew": false, "checkedAt": tag},
        "resetCredits": {"availableCount": 1, "nextExpiresAt": tag},
        "resetOffer": {"price": {"amountMinorUnits": 800, "currency": "USD"}}
    })
}

fn reading(value: Value) -> Reading {
    serde_json::from_value(value).expect("a reading")
}

#[test]
fn the_remember_policy_field_by_field() {
    #[derive(Debug, Clone, Copy)]
    enum Kept {
        Remembered,
        RememberedWeekly,
        Nothing,
    }
    use Kept::{Nothing, Remembered, RememberedWeekly};
    let answered = |asked_what_was_spent, asked_about_renewal| Outcome::Answered {
        asked_what_was_spent,
        asked_about_renewal,
    };
    let outcomes = [
        answered(false, false),
        answered(true, false),
        answered(false, true),
        answered(true, true),
        Outcome::Failed,
    ];
    let table = [
        ("plan", [Nothing, Nothing, Nothing, Nothing, Remembered]),
        (
            "windows",
            [
                RememberedWeekly,
                RememberedWeekly,
                RememberedWeekly,
                RememberedWeekly,
                Remembered,
            ],
        ),
        ("credits", [Nothing, Nothing, Nothing, Nothing, Remembered]),
        (
            "workspaceCredits",
            [Nothing, Nothing, Nothing, Nothing, Remembered],
        ),
        (
            "creditsSpent",
            [Nothing, Remembered, Nothing, Remembered, Remembered],
        ),
        (
            "subscription",
            [Nothing, Nothing, Remembered, Remembered, Remembered],
        ),
        ("resetCredits", [Remembered; 5]),
        ("resetOffer", [Nothing; 5]),
    ];
    let (own, remembered) = (every_field("own", 1.0), every_field("remembered", 2.0));
    for (field, cells) in table {
        for (outcome, cell) in outcomes.into_iter().zip(cells) {
            let mut unreported = own.clone();
            if field == "windows" {
                unreported[field] = json!([own[field][1]]);
            } else {
                unreported.as_object_mut().unwrap().remove(field);
            }
            let kept = serde_json::to_value(
                reading(unreported).keeping(reading(remembered.clone()), outcome),
            )
            .unwrap();
            let (own_session, remembered_weekly) = (&own["windows"][1], &remembered["windows"][0]);
            let expected = match (field, cell) {
                ("windows", RememberedWeekly) => Some(json!([remembered_weekly, own_session])),
                ("windows", Remembered) => Some(json!([
                    remembered_weekly,
                    own_session,
                    remembered["windows"][1]
                ])),
                ("windows", Nothing) => Some(json!([own_session])),
                (_, Remembered) => Some(remembered[field].clone()),
                (_, RememberedWeekly) => unreachable!("only windows keep the remembered weekly"),
                (_, Nothing) => None,
            };
            assert_eq!(
                kept.get(field).cloned(),
                expected,
                "{field} not reported, {outcome:?}"
            );

            let kept = serde_json::to_value(
                reading(own.clone()).keeping(reading(remembered.clone()), outcome),
            )
            .unwrap();
            let expected = match (field, outcome) {
                ("windows", Outcome::Failed) => json!([
                    own["windows"][0],
                    remembered["windows"][0],
                    own["windows"][1],
                    remembered["windows"][1]
                ]),
                _ => own[field].clone(),
            };
            assert_eq!(kept[field], expected, "{field} reported, {outcome:?}");
        }
    }
}
