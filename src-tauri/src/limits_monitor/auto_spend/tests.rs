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

/// Whatever changed in the ten minutes, a reset the account no longer needs is not spent.
#[test]
fn at_its_time_a_reset_no_longer_needed_is_not_spent() {
    let later_cycle = "2026-10-08T12:00:00Z";
    let signed_out = {
        let mut card = card(96.0, RENEWS);
        card.current_account = false;
        card
    };
    for (why, snapshots) in [
        ("more than its share is left", vec![card(50.0, RENEWS)]),
        ("a new weekly cycle began", vec![card(96.0, later_cycle)]),
        ("another account is signed in", vec![signed_out]),
        ("nothing was read", Vec::new()),
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
fn a_cancelled_spend_is_gone_and_a_second_cancel_says_so() {
    let mut pending = scheduled();

    assert!(cancel(&mut pending, "acct"));
    assert!(!cancel(&mut pending, "acct"));
    assert!(due(
        &mut pending,
        &[card(96.0, RENEWS)],
        &automatic(),
        minutes_after_now(10)
    )
    .is_empty());
}

#[test]
fn the_monitor_wakes_for_the_soonest_spend() {
    let mut pending = scheduled();
    let mut later = schedule(&offer(), minutes_after_now(5));
    later.account_id = "other".into();
    pending.insert("other".into(), later);

    assert_eq!(next_due(&pending), Some(minutes_after_now(10)));
    assert_eq!(next_due(&HashMap::new()), None);
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
    assert_eq!(
        outcome_copy(&spend, &Err("Codex is not signed in.".to_string())),
        (
            "Codex: banked reset not used".to_string(),
            "Codex is not signed in.".to_string()
        )
    );
    assert_eq!(
        not_needed_copy(&spend),
        (
            "Codex: banked reset not used".to_string(),
            "you@example.com no longer needs it, or is no longer signed in, so it was kept."
                .to_string()
        )
    );
}

/// The card lists what is waiting, when it is due, and cancels it; a cancel tells every window.
#[test]
fn the_card_lists_a_waiting_spend_and_cancels_it() {
    let spend = schedule(&offer(), at(NOW));
    with_pending(|pending| pending.insert(spend.account_id.clone(), spend));
    let _ = crate::read_revision::take_announced();

    assert_eq!(
        listed(),
        [PendingResetSpendDto {
            account_id: "acct".into(),
            due_at: "2026-10-01T12:10:00Z".into(),
        }]
    );
    assert!(cancel_listed("acct"));
    assert_eq!(
        crate::read_revision::take_announced(),
        [crate::read_revision::Source::ResetSpends]
    );
    assert!(listed().is_empty());
    assert!(!cancel_listed("acct"));
    assert!(
        crate::read_revision::take_announced().is_empty(),
        "announced a cancel of nothing"
    );
}
