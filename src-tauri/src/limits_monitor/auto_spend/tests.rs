//! When an automatic alert's reset is spent: ten minutes after it is offered, only if the account
//! still needs it in the same weekly cycle, and never once it is cancelled or turned off.
use super::*;
use crate::dto::{AgentId, LimitWindowDto, LimitWindowKind, LimitsResetCreditsDto, Reading};

const NOW: &str = "2026-10-01T12:00:00Z";
const RENEWS: &str = "2026-10-05T12:00:00Z";

fn at(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn minutes_after_now(minutes: i64) -> DateTime<Utc> {
    at(NOW) + chrono::Duration::minutes(minutes)
}

/// The signed-in Codex card of `acct`, `used` of its week used, renewing at `renews`.
fn card(used: f64, renews: &str) -> ProviderLimitsDto {
    ProviderLimitsDto::for_test(AgentId::Codex, "acct")
        .labelled("you@example.com")
        .with_reading(Reading {
            windows: vec![LimitWindowDto {
                id: "weekly".into(),
                label: "Weekly".into(),
                kind: LimitWindowKind::Weekly,
                used_percent: used,
                resets_at: Some(renews.into()),
                window_seconds: Some(604_800),
                observed_at: NOW.into(),
            }],
            reset_credits: Some(LimitsResetCreditsDto {
                available_count: 1,
                next_expires_at: None,
                resets: Vec::new(),
            }),
            ..Reading::default()
        })
}

fn automatic() -> HashMap<String, ResetAlert> {
    HashMap::from([(
        "acct".to_string(),
        ResetAlert {
            label: None,
            max_left_percent: 10,
            min_hours_to_renewal: 24,
            automatic: true,
        },
    )])
}

fn offer() -> Offer {
    Offer {
        account_id: "acct".into(),
        account_label: Some("you@example.com".into()),
        cycle: RENEWS.into(),
        left_percent: 5.0,
        renews_at: at(RENEWS),
        available: 1,
        automatic: true,
    }
}

fn scheduled() -> HashMap<String, PendingSpend> {
    let spend = schedule(&offer(), at(NOW));
    HashMap::from([(spend.account_id.clone(), spend)])
}

#[test]
fn an_offer_is_spent_ten_minutes_later_under_its_own_key() {
    let spend = schedule(&offer(), at(NOW));

    assert_eq!(spend.account_id, "acct");
    assert_eq!(spend.cycle, RENEWS);
    assert_eq!(spend.due_at, minutes_after_now(10));
    assert!(!spend.idempotency_key.is_empty());
    assert_ne!(
        spend.idempotency_key,
        schedule(&offer(), at(NOW)).idempotency_key,
        "two spends share a key"
    );
}

#[test]
fn nothing_is_spent_before_its_time() {
    let mut pending = scheduled();

    let due = due(
        &mut pending,
        &[card(96.0, RENEWS)],
        &automatic(),
        minutes_after_now(9),
    );

    assert!(due.is_empty());
    assert_eq!(pending.len(), 1);
}

#[test]
fn at_its_time_a_reset_still_needed_is_spent_and_leaves_the_queue() {
    let mut pending = scheduled();
    let expected = pending["acct"].clone();

    let due = due(
        &mut pending,
        &[card(96.0, RENEWS)],
        &automatic(),
        minutes_after_now(10),
    );

    assert_eq!(due, [Due::Spend(expected)]);
    assert!(pending.is_empty());
}

/// A reset the live read at its time no longer finds needed is not spent.
#[test]
fn at_its_time_a_reset_no_longer_needed_is_not_spent() {
    let later_cycle = "2026-10-08T12:00:00Z";
    let someone_else = {
        let mut other = card(96.0, RENEWS);
        other.account.as_mut().unwrap().id = "other".into();
        other
    };
    for (why, snapshots) in [
        ("more than its share is left", vec![card(50.0, RENEWS)]),
        ("a new weekly cycle began", vec![card(96.0, later_cycle)]),
        ("another account is signed in", vec![someone_else]),
    ] {
        let mut pending = scheduled();
        let expected = pending["acct"].clone();

        let due = due(
            &mut pending,
            &snapshots,
            &automatic(),
            minutes_after_now(10),
        );

        assert_eq!(due, [Due::NotNeeded(expected)], "{why}");
        assert!(pending.is_empty(), "{why}");
    }
}

/// Without a live read of the signed-in account nothing is known about now, so the spend waits for
/// a read that answers.
#[test]
fn without_a_live_read_a_spend_waits() {
    let failed = {
        let mut card = card(96.0, RENEWS);
        card.status = crate::dto::LimitsStatus::Failed;
        card
    };
    for snapshots in [vec![failed], Vec::new()] {
        let mut pending = scheduled();

        let due = due(
            &mut pending,
            &snapshots,
            &automatic(),
            minutes_after_now(12),
        );

        assert!(due.is_empty());
        assert_eq!(pending.len(), 1);
    }
}

/// A spend found more than fifteen minutes past its time, as after the computer slept, is kept
/// even when the account still needs it: the user was told too long ago.
#[test]
fn a_spend_found_late_is_kept() {
    for snapshots in [vec![card(96.0, RENEWS)], Vec::new()] {
        let mut pending = scheduled();
        let expected = pending["acct"].clone();

        let due = due(
            &mut pending,
            &snapshots,
            &automatic(),
            minutes_after_now(26),
        );

        assert_eq!(due, [Due::Late(expected)]);
        assert!(pending.is_empty());
    }
}

#[test]
fn fifteen_minutes_past_its_time_a_spend_is_still_made() {
    let mut pending = scheduled();

    let due = due(
        &mut pending,
        &[card(96.0, RENEWS)],
        &automatic(),
        minutes_after_now(25),
    );

    assert!(matches!(due.as_slice(), [Due::Spend(_)]));
}

/// An alert turned off, or turned back to notifying only, takes its waiting spend with it.
#[test]
fn a_spend_whose_alert_no_longer_spends_is_dropped_without_a_word() {
    let mut notify_only = automatic();
    notify_only.get_mut("acct").unwrap().automatic = false;
    for alerts in [HashMap::new(), notify_only] {
        let mut pending = scheduled();

        let due = due(
            &mut pending,
            &[card(96.0, RENEWS)],
            &alerts,
            minutes_after_now(10),
        );

        assert!(due.is_empty());
        assert!(pending.is_empty());
    }
}

#[test]
fn the_notifications_say_when_and_what_came_of_it() {
    let spend = schedule(&offer(), at(NOW));

    assert_eq!(
        scheduled_copy(&offer()),
        (
            "Codex: using a banked reset in 10 minutes".to_string(),
            "you@example.com has 5% of its limit left. Cancel on the account's card in on-n-off to keep the reset.".to_string()
        )
    );
    assert_eq!(
        outcome_copy(&spend, &Ok(ResetCreditOutcome::Reset)),
        (
            "Codex: banked reset used".to_string(),
            "you@example.com's usage is back to 0%.".to_string()
        )
    );
    for (outcome, body) in [
        (ResetCreditOutcome::NothingToReset, "you@example.com's usage was already at 0%."),
        (ResetCreditOutcome::NoCredit, "you@example.com has no banked reset left."),
        (ResetCreditOutcome::AlreadyRedeemed, "That banked reset was already used."),
        (
            ResetCreditOutcome::Unknown,
            "Codex answered with a result on-n-off doesn't recognize. Check the reset count on the card.",
        ),
    ] {
        assert_eq!(
            outcome_copy(&spend, &Ok(outcome)),
            ("Codex: banked reset not used".to_string(), body.to_string()),
            "{outcome:?}"
        );
    }
    assert_eq!(
        outcome_copy(&spend, &Err("Codex is not signed in.".to_string())),
        (
            "Codex: banked reset not used".to_string(),
            "Codex is not signed in.".to_string()
        )
    );
    assert_eq!(
        kept_copy(&Due::NotNeeded(spend.clone())),
        Some((
            "Codex: banked reset not used".to_string(),
            "you@example.com no longer needs it, or is no longer signed in, so it was kept."
                .to_string()
        ))
    );
    assert_eq!(
        kept_copy(&Due::Late(spend.clone())),
        Some((
            "Codex: banked reset not used".to_string(),
            "you@example.com could not be checked in time, so it was kept.".to_string()
        ))
    );
    assert_eq!(kept_copy(&Due::Spend(spend)), None);
}

/// The waiting spends in this app, one test so no other touches them meanwhile: scheduled once an
/// offer is saved, listed for the card, woken for, decided, cancelled from the card, and cleared,
/// each change told to every window and a change of nothing told to none.
#[test]
fn the_waiting_spends_move_through_their_life_and_every_change_is_told() {
    use crate::read_revision::{take_announced, Source};
    let _ = take_announced();
    clear();
    let _ = take_announced();

    let notices = schedule_all(&[offer()], at(NOW));
    assert_eq!(notices, [scheduled_copy(&offer())]);
    assert_eq!(take_announced(), [Source::ResetSpends]);
    assert!(schedule_all(&[], at(NOW)).is_empty());
    assert!(take_announced().is_empty(), "announced scheduling nothing");
    assert_eq!(
        listed(),
        [PendingResetSpendDto {
            account_id: "acct".into(),
            due_at: "2026-10-01T12:10:00Z".into(),
        }]
    );
    assert_eq!(next_due(), Some(minutes_after_now(10)));
    assert!(!any_due(minutes_after_now(9)));
    assert!(any_due(minutes_after_now(10)));

    assert!(take_due(&[card(96.0, RENEWS)], &automatic(), minutes_after_now(9)).is_empty());
    assert!(take_announced().is_empty(), "announced deciding nothing");

    assert!(cancel_listed("acct"));
    assert_eq!(take_announced(), [Source::ResetSpends]);
    assert!(!cancel_listed("acct"));
    assert!(take_announced().is_empty(), "announced a cancel of nothing");
    assert!(listed().is_empty());

    schedule_all(&[offer()], at(NOW));
    let _ = take_announced();
    assert!(matches!(
        take_due(&[card(96.0, RENEWS)], &automatic(), minutes_after_now(10)).as_slice(),
        [Due::Spend(_)]
    ));
    assert_eq!(take_announced(), [Source::ResetSpends]);
    assert_eq!(next_due(), None);

    schedule_all(&[offer()], at(NOW));
    let _ = take_announced();
    assert!(clear());
    assert_eq!(take_announced(), [Source::ResetSpends]);
    assert!(!clear());
    assert!(take_announced().is_empty(), "announced clearing nothing");
}
