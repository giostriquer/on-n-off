use super::*;

fn minutes_after_now(minutes: i64) -> DateTime<Utc> {
    now() + chrono::Duration::minutes(minutes)
}

fn live(used: f64, renews: &str) -> ProviderLimitsDto {
    card("acct", used, renews, NOW, 1)
}

fn automatic() -> HashMap<String, ResetAlert> {
    alerts_for("acct", alert(5, true))
}

fn offer() -> Offer {
    Offer {
        account_id: "acct".into(),
        account_label: Some("you@example.com".into()),
        left_percent: 5.0,
        renews_at: at(RENEWS),
        automatic: true,
    }
}

fn scheduled() -> Waiting {
    let mut waiting = Waiting::default();
    waiting.schedule(&[offer()], now());
    waiting
}

const NOT_NEEDED: &str = "no longer needs it, or is no longer signed in";
const LATE: &str = "could not be checked in time";

fn kept(why: &str) -> Due {
    Due::Kept((
        "Codex: banked reset not used".to_string(),
        format!("you@example.com {why}, so it was kept."),
    ))
}

fn falls_due(
    waiting: &mut Waiting,
    readings: &[ProviderLimitsDto],
    alerts: &HashMap<String, ResetAlert>,
    at: DateTime<Utc>,
) -> Vec<Due> {
    plan(&mut AlertMemory::default(), waiting, readings, alerts, at).due
}

#[test]
fn an_offer_is_spent_ten_minutes_later_under_its_own_key() {
    let waiting = scheduled();
    let spend = &waiting.0["acct"];

    assert_eq!(spend.account_id(), "acct");
    assert_eq!(spend.cycle, ResetInstant::from(at(RENEWS)));
    assert_eq!(spend.due_at, minutes_after_now(10));
    assert_eq!(spend.idempotency_key(), spend.idempotency_key);
    assert!(uuid::Uuid::parse_str(spend.idempotency_key()).is_ok());
    assert_ne!(
        spend.idempotency_key(),
        scheduled().0["acct"].idempotency_key(),
        "two spends share a key"
    );
}

#[test]
fn nothing_is_spent_before_its_time() {
    let mut waiting = scheduled();

    let due = falls_due(
        &mut waiting,
        &[live(96.0, RENEWS)],
        &automatic(),
        minutes_after_now(9),
    );

    assert!(due.is_empty());
    assert_eq!(waiting.0.len(), 1);
}

#[test]
fn at_its_time_a_reset_still_needed_is_spent_and_leaves_the_queue() {
    let mut waiting = scheduled();
    let expected = waiting.0["acct"].clone();

    let due = falls_due(
        &mut waiting,
        &[live(96.0, RENEWS)],
        &automatic(),
        minutes_after_now(10),
    );

    assert_eq!(due, [Due::Spend(expected, SpendShare(5))]);
    assert!(waiting.0.is_empty());
}

#[test]
fn a_spend_is_held_to_its_alerts_lower_share() {
    let mut waiting = scheduled();

    let due = falls_due(
        &mut waiting,
        &[live(97.0, RENEWS)],
        &alerts_for("acct", alert(3, true)),
        minutes_after_now(10),
    );

    assert!(
        matches!(due.as_slice(), [Due::Spend(_, SpendShare(3))]),
        "{due:?}"
    );
}

#[test]
fn at_its_time_a_reset_no_longer_needed_is_not_spent() {
    let later_cycle = "2026-10-08T12:00:00Z";
    let someone_else = card("other", 96.0, RENEWS, NOW, 1);
    for (why, readings) in [
        ("more than its share is left", vec![live(50.0, RENEWS)]),
        ("a new weekly cycle began", vec![live(96.0, later_cycle)]),
        ("another account is signed in", vec![someone_else]),
    ] {
        let mut waiting = scheduled();

        let due = falls_due(&mut waiting, &readings, &automatic(), minutes_after_now(10));

        assert_eq!(due, [kept(NOT_NEEDED)], "{why}");
        assert!(waiting.0.is_empty(), "{why}");
    }
}

#[test]
fn the_signed_in_card_of_a_provider_that_banks_resets_decides_among_the_others() {
    let mut readings = vec![ProviderLimitsDto::for_test(
        without_banked_resets(),
        "other-acct",
    )];
    readings.push(ProviderLimitsDto {
        current_account: false,
        ..card("remembered", 20.0, RENEWS, NOW, 1)
    });
    readings.push(live(96.0, RENEWS));
    let mut waiting = scheduled();

    let due = falls_due(&mut waiting, &readings, &automatic(), minutes_after_now(10));

    assert!(matches!(due.as_slice(), [Due::Spend(..)]), "{due:?}");
}

#[test]
fn nobody_signed_in_keeps_the_reset() {
    for status in [LimitsStatus::SignedOut, LimitsStatus::Unsupported] {
        let mut signed_out = live(96.0, RENEWS);
        signed_out.status = status;
        signed_out.account = None;
        let mut waiting = scheduled();

        let due = falls_due(
            &mut waiting,
            &[signed_out],
            &automatic(),
            minutes_after_now(10),
        );

        assert_eq!(due, [kept(NOT_NEEDED)], "{status:?}");
    }
}

#[test]
fn without_a_live_read_a_spend_waits() {
    let failed = ProviderLimitsDto {
        status: LimitsStatus::Failed,
        ..live(96.0, RENEWS)
    };
    for readings in [vec![failed], Vec::new()] {
        let mut waiting = scheduled();

        let due = falls_due(&mut waiting, &readings, &automatic(), minutes_after_now(12));

        assert!(due.is_empty());
        assert_eq!(waiting.0.len(), 1);
    }
}

#[test]
fn a_spend_found_late_is_kept() {
    for readings in [vec![live(96.0, RENEWS)], Vec::new()] {
        let mut waiting = scheduled();

        let due = falls_due(&mut waiting, &readings, &automatic(), minutes_after_now(26));

        assert_eq!(due, [kept(LATE)]);
        assert!(waiting.0.is_empty());
    }
}

#[test]
fn fifteen_minutes_past_its_time_a_spend_is_still_made() {
    let mut waiting = scheduled();

    let due = falls_due(
        &mut waiting,
        &[live(96.0, RENEWS)],
        &automatic(),
        minutes_after_now(25),
    );

    assert!(matches!(due.as_slice(), [Due::Spend(..)]));
}

#[test]
fn a_spend_whose_alert_no_longer_spends_is_dropped_without_a_word() {
    for alerts in [HashMap::new(), alerts_for("acct", alert(5, false))] {
        let mut waiting = scheduled();

        let due = falls_due(
            &mut waiting,
            &[live(96.0, RENEWS)],
            &alerts,
            minutes_after_now(10),
        );

        assert!(due.is_empty());
        assert!(waiting.0.is_empty());
    }
}

#[test]
fn the_waiting_spends_are_listed_found_due_ended_and_cleared() {
    let mut waiting = scheduled();
    assert_eq!(
        waiting.listed(),
        [PendingResetSpendDto {
            account_id: "acct".into(),
            due_at: "2026-10-01T12:10:00Z".into(),
        }]
    );
    assert_eq!(
        serde_json::to_value(waiting.listed()).unwrap(),
        serde_json::json!([{"accountId": "acct", "dueAt": "2026-10-01T12:10:00Z"}])
    );
    assert_eq!(waiting.next_due(), Some(minutes_after_now(10)));
    assert!(!waiting.any_due(minutes_after_now(9)));
    assert!(waiting.any_due(minutes_after_now(10)));

    assert!(waiting.cancel("acct"));
    assert!(!waiting.cancel("acct"));
    assert!(waiting.listed().is_empty());
    assert_eq!(waiting.next_due(), None);

    let mut waiting = scheduled();
    waiting.used_by_hand("acct", ResetCreditOutcome::NothingToReset);
    waiting.used_by_hand("other", ResetCreditOutcome::Reset);
    assert_eq!(waiting.listed().len(), 1);
    waiting.used_by_hand("acct", ResetCreditOutcome::Reset);
    assert!(waiting.listed().is_empty());

    let mut waiting = scheduled();
    waiting.clear();
    assert_eq!(waiting, Waiting::default());
}

#[test]
fn the_notifications_say_when_and_what_came_of_it() {
    let mut waiting = Waiting::default();

    assert_eq!(
        waiting.schedule(&[offer()], now()),
        [(
            "Codex: using a banked reset in 10 minutes".to_string(),
            "you@example.com has 5% of its limit left. Cancel on the account's card in on-n-off to keep the reset.".to_string()
        )]
    );
    let spend = &waiting.0["acct"];
    assert_eq!(
        spend.outcome_notice(&Ok(ResetCreditOutcome::Reset)),
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
            spend.outcome_notice(&Ok(outcome)),
            ("Codex: banked reset not used".to_string(), body.to_string()),
            "{outcome:?}"
        );
    }
    assert_eq!(
        spend.outcome_notice(&Err("Codex is not signed in.".to_string())),
        (
            "Codex: banked reset not used".to_string(),
            "Codex is not signed in.".to_string()
        )
    );
    assert_eq!(
        spend.kept_notice(Kept::NotNeeded),
        (
            "Codex: banked reset not used".to_string(),
            "you@example.com no longer needs it, or is no longer signed in, so it was kept."
                .to_string()
        )
    );
    assert_eq!(
        spend.kept_notice(Kept::Late),
        (
            "Codex: banked reset not used".to_string(),
            "you@example.com could not be checked in time, so it was kept.".to_string()
        )
    );
}

#[test]
fn an_account_without_a_label_is_named_for_what_it_is() {
    let unlabelled = Offer {
        account_label: None,
        ..offer()
    };
    let mut waiting = Waiting::default();

    let scheduled = waiting.schedule(std::slice::from_ref(&unlabelled), now());

    assert_eq!(
        scheduled[0].1,
        "The Codex account has 5% of its limit left. Cancel on the account's card in on-n-off to keep the reset."
    );
    assert_eq!(
        waiting.0["acct"].kept_notice(Kept::Late).1,
        "The Codex account could not be checked in time, so it was kept."
    );
    assert_eq!(
        unlabelled.notice(now()).1,
        "Codex has 5% of its limit left and renews in 4d 0h. Open on-n-off to use a banked reset."
    );
}

#[test]
fn a_spend_whose_reset_time_reads_a_second_or_two_apart_is_still_made() {
    for renews in [
        "2026-10-05T12:00:01Z",
        "2026-10-05T11:59:58+00:00",
        "2026-10-05T12:00:00+00:00",
        "2026-10-05T12:00:01.000Z",
        "2026-10-05T11:59:59.692Z",
    ] {
        let mut waiting = scheduled();

        let due = falls_due(
            &mut waiting,
            &[live(96.0, renews)],
            &automatic(),
            minutes_after_now(10),
        );

        assert!(
            matches!(due.as_slice(), [Due::Spend(..)]),
            "{renews}: {due:?}"
        );
    }
}

#[test]
fn a_poll_tells_its_notify_only_offers_before_the_spends_it_schedules() {
    let mut waiting = Waiting::default();
    let offered = Offered {
        told: vec![(
            "Codex: a banked reset is available".to_string(),
            "told".to_string(),
        )],
        automatic: vec![offer()],
    };

    let notices = offered.commit(&mut waiting, now());

    assert_eq!(
        notices
            .iter()
            .map(|(title, _)| title.as_str())
            .collect::<Vec<_>>(),
        [
            "Codex: a banked reset is available",
            "Codex: using a banked reset in 10 minutes"
        ]
    );
    assert_eq!(waiting.next_due(), Some(minutes_after_now(10)));
}

#[test]
fn the_next_spend_due_is_the_earliest_of_those_waiting() {
    let for_account = |account: &str| Offer {
        account_id: account.into(),
        ..offer()
    };
    let mut waiting = Waiting::default();
    waiting.schedule(&[for_account("middle")], minutes_after_now(3));
    waiting.schedule(&[for_account("latest")], minutes_after_now(7));
    waiting.schedule(&[for_account("earliest")], now());

    assert_eq!(waiting.next_due(), Some(minutes_after_now(10)));
    assert!(!waiting.any_due(minutes_after_now(10) - chrono::Duration::seconds(1)));
    assert!(waiting.any_due(minutes_after_now(10)));
}
