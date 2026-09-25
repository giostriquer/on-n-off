use super::*;
use crate::dto::{AgentId, LimitWindowKind, LimitsSubscriptionDto};
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

/// What a paused read shows of `remembered` when it read `own`.
fn paused(own: Reading, remembered: Reading) -> Reading {
    own.keeping(remembered, Outcome::Failed)
}

#[test]
fn newer_windows_merge_independently_and_do_not_inherit_an_old_reset() {
    let own = Reading {
        windows: vec![
            observed(
                "weekly_all",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                63.0,
                None,
                "2026-08-18T03:07:53.000Z",
            ),
            observed(
                "session",
                "5 hour · all models",
                LimitWindowKind::Session,
                17.0,
                None,
                "2026-08-18T03:07:53.000Z",
            ),
        ],
        ..Reading::default()
    };
    let remembered = Reading {
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
        ..Reading::default()
    };

    let merged = paused(own, remembered);
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

#[test]
fn a_paused_refresh_keeps_the_remembered_reset_credit_count() {
    let reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 1,
        next_expires_at: None,
    });
    let remembered = Reading {
        plan: Some("pro".to_string()),
        windows: vec![observed(
            "primary",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            40.0,
            None,
            "2026-08-17T15:00:00Z",
        )],
        reset_credits: reset_credits.clone(),
        // A remembered account can carry no offer, and merging must not invent one either.
        reset_offer: Some(crate::dto::LimitsResetOfferDto {
            price: Some(crate::dto::LimitsPriceDto {
                amount_minor_units: 800,
                currency: "USD".to_string(),
            }),
        }),
        ..Reading::default()
    };

    let merged = paused(Reading::default(), remembered);

    assert_eq!(merged.reset_credits, reset_credits);
    assert_eq!(merged.reset_offer, None);
}

#[test]
fn a_paused_refresh_keeps_banked_resets_remembered_without_any_windows() {
    let reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 2,
        next_expires_at: None,
    });
    let remembered = Reading {
        plan: Some("pro".to_string()),
        reset_credits: reset_credits.clone(),
        ..Reading::default()
    };

    let merged = paused(Reading::default(), remembered);

    assert_eq!(merged.reset_credits, reset_credits);
    // The banked count is remembered; a price the provider may already have withdrawn is not.
    assert_eq!(merged.reset_offer, None);
    assert_eq!(merged.plan.as_deref(), Some("pro"));
    assert!(merged.windows.is_empty());
}

/// A read that paused keeps the workspace-credit share the account last reported, as it keeps a
/// credit balance; a read that reports one replaces it.
#[test]
fn a_paused_refresh_keeps_the_remembered_workspace_credit_share() {
    let share = |used: &str| {
        Some(crate::dto::LimitsWorkspaceCreditsDto {
            limit: "25000".to_string(),
            used: used.to_string(),
            used_percent: 32.0,
            resets_at: None,
            reached: false,
        })
    };
    let read = |workspace_credits| Reading {
        workspace_credits,
        ..Reading::default()
    };

    assert_eq!(
        paused(read(None), read(share("8000"))).workspace_credits,
        share("8000")
    );
    assert_eq!(
        paused(read(share("9000")), read(share("8000"))).workspace_credits,
        share("9000")
    );
}

/// Spending is kept through a paused refresh like the other figures, and a read that answered wins.
#[test]
fn a_paused_refresh_keeps_the_remembered_credits_spent() {
    let spent = |last_7_days: f64| {
        Some(crate::dto::LimitsCreditsSpentDto {
            last_7_days,
            last_30_days: last_7_days,
            updated_at: None,
        })
    };
    let read = |credits_spent| Reading {
        credits_spent,
        ..Reading::default()
    };

    assert_eq!(
        paused(read(None), read(spent(100.0))).credits_spent,
        spent(100.0)
    );
    assert_eq!(
        paused(read(spent(250.0)), read(spent(100.0))).credits_spent,
        spent(250.0)
    );
}

/// The subscription status is account metadata, like the plan: a read that could not say keeps the
/// remembered one, and a read that did say wins.
#[test]
fn a_paused_refresh_keeps_the_remembered_subscription_status() {
    let read = |subscription_status: Option<&str>| Reading {
        subscription_status: subscription_status.map(str::to_string),
        ..Reading::default()
    };
    let remembered = || Reading {
        windows: vec![observed(
            "seven_day",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            40.0,
            None,
            "2026-08-17T10:00:00.000Z",
        )],
        ..read(Some("past_due"))
    };

    assert_eq!(
        paused(read(None), remembered())
            .subscription_status
            .as_deref(),
        Some("past_due")
    );
    assert_eq!(
        paused(read(Some("active")), remembered())
            .subscription_status
            .as_deref(),
        Some("active")
    );
}

#[test]
fn a_count_lapses_at_its_expiry_itself() {
    let expires_at = "2026-09-22T12:00:00+00:00";
    let resets = crate::dto::LimitsResetCreditsDto {
        available_count: 2,
        next_expires_at: Some(expires_at.to_string()),
    };
    let at = parse_observed_at(expires_at).unwrap();
    let expiry = resets.next_expires_at.as_deref();
    assert!(passed(expiry, at));
    assert!(!passed(expiry, at - chrono::Duration::seconds(1)));
    assert!(!passed(None, at), "no known expiry never lapses");
}

/// A read that answered without a plan says the account has none now, so the remembered plan goes.
#[test]
fn an_answered_read_without_a_plan_drops_the_remembered_one() {
    let remembered = Reading {
        plan: Some("max".to_string()),
        windows: vec![observed(
            "weekly_all",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            40.0,
            None,
            "2026-08-17T10:00:00.000Z",
        )],
        ..Reading::default()
    };
    let answered = Outcome::Answered {
        asked_what_was_spent: false,
        asked_about_renewal: false,
    };

    assert_eq!(Reading::default().keeping(remembered, answered).plan, None);
}

/// The term is kept through a paused refresh like the figures beside it, and a read that answered
/// wins.
#[test]
fn a_paused_refresh_keeps_the_remembered_term() {
    let term = |will_renew| {
        Some(LimitsSubscriptionDto {
            active_until: "2026-09-28T16:22:34Z".to_string(),
            will_renew,
            note: None,
            checked_at: "2026-09-25T12:00:00Z".to_string(),
        })
    };
    let read = |subscription| Reading {
        subscription,
        ..Reading::default()
    };
    let remembered = || Reading {
        windows: vec![observed(
            "primary",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            40.0,
            None,
            "2026-08-17T10:00:00.000Z",
        )],
        ..read(term(false))
    };

    assert_eq!(paused(read(None), remembered()).subscription, term(false));
    assert_eq!(
        paused(read(term(true)), remembered()).subscription,
        term(true)
    );
}

/// A Codex card is asked about its term, so a successful read that could not tell it keeps the
/// remembered one and one that answered replaces it; a Claude card is never asked and keeps none.
#[test]
fn a_card_keeps_the_term_it_remembers_when_a_read_could_not_tell() {
    let term = |will_renew| {
        Some(LimitsSubscriptionDto {
            active_until: "2026-09-28T16:22:34Z".to_string(),
            will_renew,
            note: None,
            checked_at: "2026-09-25T12:00:00Z".to_string(),
        })
    };
    let card = |provider, subscription| {
        ProviderLimitsDto::for_test(provider, "acct-1").with_reading(Reading {
            plan: Some("team".into()),
            subscription,
            ..Reading::default()
        })
    };
    let remembered = || card(AgentId::Codex, term(false)).reading;

    let mut silent = card(AgentId::Codex, None);
    keep_remembered(&mut silent, remembered());
    assert_eq!(silent.reading.subscription, term(false));

    let mut answered = card(AgentId::Codex, term(true));
    keep_remembered(&mut answered, remembered());
    assert_eq!(answered.reading.subscription, term(true));

    let mut claude = card(AgentId::Claude, None);
    keep_remembered(&mut claude, remembered());
    assert_eq!(claude.reading.subscription, None);
}

/// A reading with every field known, each told apart by `tag`, as wire JSON.
fn every_field(tag: &str, used: f64) -> Value {
    json!({
        "plan": tag,
        "subscriptionStatus": tag,
        "windows": [{"id": tag, "label": tag, "kind": "weekly", "usedPercent": used,
                     "observedAt": "2026-08-17T10:00:00.000Z"}],
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

/// The whole policy in one table: for each field and each way a read can go, what the reading
/// keeps where the read did not report the field. Where it did, its own value always stands (a
/// failed read's own windows are merged with the remembered ones instead).
#[test]
fn the_remember_policy_field_by_field() {
    #[derive(Debug, Clone, Copy)]
    enum Kept {
        Remembered,
        Nothing,
    }
    use Kept::{Nothing, Remembered};
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
    // Columns follow `outcomes`: answered and asked nothing, asked what was spent, asked about
    // renewal, asked both; failed.
    let table = [
        ("plan", [Nothing, Nothing, Nothing, Nothing, Remembered]),
        (
            "subscriptionStatus",
            [Nothing, Nothing, Nothing, Nothing, Remembered],
        ),
        ("windows", [Nothing, Nothing, Nothing, Nothing, Remembered]),
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
                unreported[field] = json!([]);
            } else {
                unreported.as_object_mut().unwrap().remove(field);
            }
            let kept = serde_json::to_value(
                reading(unreported).keeping(reading(remembered.clone()), outcome),
            )
            .unwrap();
            let expected = match cell {
                Remembered => Some(remembered[field].clone()),
                Nothing if field == "windows" => Some(json!([])),
                Nothing => None,
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
                ("windows", Outcome::Failed) => {
                    json!([own["windows"][0], remembered["windows"][0]])
                }
                _ => own[field].clone(),
            };
            assert_eq!(kept[field], expected, "{field} reported, {outcome:?}");
        }
    }
}
