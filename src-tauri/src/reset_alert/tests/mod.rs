mod due;
mod offers;

use super::*;
use crate::dto::{LimitWindowDto, LimitsResetCreditsDto};

const NOW: &str = "2026-10-01T12:00:00Z";
const RENEWS: &str = "2026-10-05T12:00:00Z";

fn at(value: &str) -> DateTime<Utc> {
    instant(value).unwrap()
}

fn now() -> DateTime<Utc> {
    at(NOW)
}

fn window(kind: LimitWindowKind, used: f64, resets_at: &str) -> LimitWindowDto {
    LimitWindowDto {
        id: format!("{kind:?}").to_lowercase(),
        label: format!("{kind:?}"),
        kind,
        used_percent: used,
        resets_at: Some(resets_at.into()),
        window_seconds: Some(604_800),
        observed_at: NOW.into(),
    }
}

fn card(
    account: &str,
    used: f64,
    renews: &str,
    observed_at: &str,
    banked: u32,
) -> ProviderLimitsDto {
    ProviderLimitsDto::for_test(AgentId::Codex, account)
        .labelled("you@example.com")
        .with_reading(Reading {
            windows: vec![LimitWindowDto {
                observed_at: observed_at.into(),
                ..window(LimitWindowKind::Weekly, used, renews)
            }],
            reset_credits: Some(LimitsResetCreditsDto {
                available_count: banked,
                next_expires_at: None,
                resets: Vec::new(),
            }),
            ..Reading::default()
        })
}

fn alert(max_left_percent: u8, automatic: bool) -> ResetAlert {
    ResetAlert {
        max_left_percent,
        automatic,
        ..ResetAlert::default()
    }
}

fn alerts_for(account: &str, alert: ResetAlert) -> HashMap<String, ResetAlert> {
    HashMap::from([(account.to_string(), alert)])
}

#[test]
fn a_new_alert_notifies_at_five_percent_left_a_day_or_more_before_renewal() {
    assert_eq!(
        ResetAlert::default(),
        ResetAlert {
            label: None,
            max_left_percent: 5,
            min_hours_to_renewal: 24,
            automatic: false,
        }
    );
}

#[test]
fn an_alert_spends_at_its_own_share_and_never_above_five_percent() {
    assert_eq!(alert(3, true).spend_share(), SpendShare(3));
    assert_eq!(alert(5, true).spend_share(), SpendShare(5));
    assert_eq!(alert(10, true).spend_share(), SpendShare(5));
}

#[test]
fn an_alert_is_kept_between_one_and_five_percent_and_at_most_a_week_from_renewal() {
    for (left, hours, kept) in [
        (0, 0, (1, 0)),
        (1, 24, (1, 24)),
        (3, 168, (3, 168)),
        (5, 169, (5, 168)),
        (40, u16::MAX, (5, 168)),
    ] {
        let mut alert = ResetAlert {
            min_hours_to_renewal: hours,
            ..alert(left, false)
        };

        alert.keep_within_bounds();

        assert_eq!(
            (alert.max_left_percent, alert.min_hours_to_renewal),
            kept,
            "{left}% and {hours}h"
        );
    }
}

#[test]
fn a_share_allows_a_spend_at_or_below_it_and_says_how_much_is_left_above_it() {
    let reading = |used| Reading {
        windows: vec![window(LimitWindowKind::Weekly, used, RENEWS)],
        ..Reading::default()
    };
    let share = alert(5, true).spend_share();

    assert_eq!(share.allows(&reading(95.0)), Ok(()));
    assert_eq!(share.allows(&reading(100.0)), Ok(()));
    assert_eq!(
        share.allows(&reading(94.4)),
        Err(
            "A banked reset can be used once 5% or less of the current limit is left, and 6% is left."
                .to_string()
        )
    );
    assert_eq!(
        alert(3, true).spend_share().allows(&reading(96.0)),
        Err(
            "A banked reset can be used once 3% or less of the current limit is left, and 4% is left."
                .to_string()
        )
    );
}

#[test]
fn the_fullest_main_window_says_how_much_is_left_and_a_models_own_limit_never_does() {
    let share = alert(5, true).spend_share();
    let with = |windows| Reading {
        windows,
        ..Reading::default()
    };

    assert_eq!(
        share.allows(&with(vec![
            window(LimitWindowKind::Session, 96.0, RENEWS),
            window(LimitWindowKind::Weekly, 40.0, RENEWS),
        ])),
        Ok(())
    );
    assert_eq!(
        share.allows(&with(vec![
            window(LimitWindowKind::Weekly, 40.0, RENEWS),
            window(LimitWindowKind::Model, 99.0, RENEWS),
        ])),
        Err(
            "A banked reset can be used once 5% or less of the current limit is left, and 60% is left."
                .to_string()
        )
    );
    assert_eq!(
        share.allows(&with(vec![window(LimitWindowKind::Model, 99.0, RENEWS)])),
        Err(
            "on-n-off can't tell how much of the Codex limit is left, so it won't spend a reset."
                .to_string()
        )
    );
}

#[test]
fn the_numbers_the_ui_restates_are_the_ones_the_rule_keeps() {
    let bounded = |max_left_percent, min_hours_to_renewal| {
        let mut alert = ResetAlert {
            max_left_percent,
            min_hours_to_renewal,
            ..ResetAlert::default()
        };
        alert.keep_within_bounds();
        (alert.max_left_percent, alert.min_hours_to_renewal)
    };
    let (least_share, least_hours) = bounded(0, 0);
    let (most_share, most_hours) = bounded(u8::MAX, u16::MAX);
    let mut waiting = Waiting::default();
    waiting.schedule(
        &[Offer {
            account_id: "acct".into(),
            account_label: None,
            cycle: RENEWS.into(),
            left_percent: 5.0,
            renews_at: at(RENEWS),
            automatic: true,
        }],
        now(),
    );
    let delay = waiting.next_due().unwrap() - now();
    let share = |max_left_percent| alert(max_left_percent, true).spend_share().0;
    let shares = [1, 3, 5, 6, 40].map(|left| [left, share(left)]);
    let rule = serde_json::json!({
        "spendCapPercent": share(u8::MAX),
        "autoSpendDelayMinutes": delay.num_minutes(),
        "defaultAlert": ResetAlert::default(),
        "bounds": {
            "maxLeftPercent": {"min": least_share, "max": most_share},
            "minHoursToRenewal": {"min": least_hours, "max": most_hours}
        },
        "spendShares": shares
    });

    assert_eq!(
        rule,
        serde_json::from_str::<serde_json::Value>(include_str!("../fixtures/rule.json")).unwrap(),
        "src-tauri/src/reset_alert/fixtures/rule.json must read:\n{}",
        serde_json::to_string_pretty(&rule).unwrap()
    );
}
