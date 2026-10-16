use super::*;

fn opted_in(account: &str) -> HashMap<String, ResetAlert> {
    alerts_for(account, alert(5, false))
}

fn poll(
    memory: &mut AlertMemory,
    cards: &[ProviderLimitsDto],
    alerts: &HashMap<String, ResetAlert>,
) -> Plan {
    plan(memory, &mut Waiting::default(), cards, alerts, now())
}

fn offered(
    memory: &mut AlertMemory,
    card: &ProviderLimitsDto,
    alerts: &HashMap<String, ResetAlert>,
) -> usize {
    let plan = poll(memory, std::slice::from_ref(card), alerts);
    plan.offered.told.len() + plan.offered.automatic.len()
}

fn two_polls(
    first: &ProviderLimitsDto,
    second: &ProviderLimitsDto,
    alerts: &HashMap<String, ResetAlert>,
) -> usize {
    let mut memory = AlertMemory::default();
    assert_eq!(
        offered(&mut memory, first, alerts),
        0,
        "offered on one poll"
    );
    offered(&mut memory, second, alerts)
}

#[test]
fn an_account_found_low_on_two_polls_in_a_row_is_offered_its_reset_once() {
    for automatic in [false, true] {
        let alerts = alerts_for("acct", alert(5, automatic));
        let mut memory = AlertMemory::default();
        let polls: Vec<Plan> = [
            card("acct", 95.0, RENEWS, "2026-10-01T11:50:00Z", 2),
            card("acct", 96.0, RENEWS, "2026-10-01T12:00:00Z", 2),
            card("acct", 97.0, RENEWS, "2026-10-01T12:10:00Z", 2),
        ]
        .iter()
        .map(|card| poll(&mut memory, std::slice::from_ref(card), &alerts))
        .collect();

        assert_eq!(polls[0], Plan::default(), "offered on one reading");
        let offer = Offer {
            account_id: "acct".into(),
            account_label: Some("you@example.com".into()),
            left_percent: 4.0,
            renews_at: at(RENEWS),
            automatic,
        };
        let offered = if automatic {
            Offered {
                told: Vec::new(),
                automatic: vec![offer],
            }
        } else {
            Offered {
                told: vec![offer.notice(now())],
                automatic: Vec::new(),
            }
        };
        let expected = Plan {
            offered,
            due: Vec::new(),
        };
        assert_eq!(polls[1], expected, "automatic: {automatic}");
        assert_eq!(
            polls[2],
            Plan::default(),
            "offered twice in one weekly cycle"
        );
    }
}

#[test]
fn the_same_reading_twice_is_not_two_polls() {
    let low = card("acct", 95.0, RENEWS, "2026-10-01T11:50:00Z", 1);

    assert_eq!(two_polls(&low, &low, &opted_in("acct")), 0);
}

#[test]
fn no_reset_is_offered_while_more_than_the_share_is_left() {
    let first = card("acct", 85.0, RENEWS, "2026-10-01T11:50:00Z", 1);
    let second = card("acct", 94.0, RENEWS, "2026-10-01T12:00:00Z", 1);

    assert_eq!(two_polls(&first, &second, &opted_in("acct")), 0);
}

#[test]
fn exactly_the_share_left_is_low() {
    let first = card("acct", 95.0, RENEWS, "2026-10-01T11:50:00Z", 1);
    let second = card("acct", 95.0, RENEWS, "2026-10-01T12:00:00Z", 1);

    assert_eq!(two_polls(&first, &second, &opted_in("acct")), 1);
}

#[test]
fn no_reset_is_offered_when_the_limit_renews_soon_anyway() {
    let soon = "2026-10-02T11:00:00Z";
    let first = card("acct", 99.0, soon, "2026-10-01T11:50:00Z", 1);
    let second = card("acct", 99.0, soon, "2026-10-01T12:00:00Z", 1);

    assert_eq!(two_polls(&first, &second, &opted_in("acct")), 0);
}

#[test]
fn no_reset_is_offered_without_one_to_spend() {
    let first = card("acct", 99.0, RENEWS, "2026-10-01T11:50:00Z", 0);
    let second = card("acct", 99.0, RENEWS, "2026-10-01T12:00:00Z", 0);

    assert_eq!(two_polls(&first, &second, &opted_in("acct")), 0);
}

#[test]
fn an_account_not_opted_in_is_never_offered_a_reset() {
    let first = card("acct", 99.0, RENEWS, "2026-10-01T11:50:00Z", 1);
    let second = card("acct", 99.0, RENEWS, "2026-10-01T12:00:00Z", 1);

    assert_eq!(two_polls(&first, &second, &opted_in("someone-else")), 0);
}

#[test]
fn only_the_signed_in_accounts_live_read_counts() {
    for change in [
        |card: &mut ProviderLimitsDto| card.current_account = false,
        |card: &mut ProviderLimitsDto| card.status = LimitsStatus::Failed,
        |card: &mut ProviderLimitsDto| card.provider = AgentId::Claude,
    ] {
        let mut first = card("acct", 99.0, RENEWS, "2026-10-01T11:50:00Z", 1);
        let mut second = card("acct", 99.0, RENEWS, "2026-10-01T12:00:00Z", 1);
        change(&mut first);
        change(&mut second);

        assert_eq!(two_polls(&first, &second, &opted_in("acct")), 0);
    }
}

#[test]
fn a_poll_that_is_not_low_starts_the_count_again() {
    let alerts = opted_in("acct");
    let mut memory = AlertMemory::default();
    for (used, observed_at) in [
        (95.0, "2026-10-01T11:40:00Z"),
        (50.0, "2026-10-01T11:50:00Z"),
        (95.0, "2026-10-01T12:00:00Z"),
    ] {
        let offers = offered(
            &mut memory,
            &card("acct", used, RENEWS, observed_at, 1),
            &alerts,
        );
        assert_eq!(offers, 0, "{used} at {observed_at}");
    }
}

#[test]
fn a_new_weekly_cycle_can_be_offered_again() {
    let alerts = opted_in("acct");
    let later = "2026-10-08T12:00:00Z";
    let mut memory = AlertMemory::default();
    let offers: Vec<usize> = [
        (RENEWS, "2026-10-01T11:40:00Z"),
        (RENEWS, "2026-10-01T11:50:00Z"),
        (later, "2026-10-01T12:00:00Z"),
        (later, "2026-10-01T12:10:00Z"),
    ]
    .into_iter()
    .map(|(renews, observed_at)| {
        offered(
            &mut memory,
            &card("acct", 95.0, renews, observed_at, 1),
            &alerts,
        )
    })
    .collect();

    assert_eq!(offers, [0, 1, 0, 1]);
}

#[test]
fn a_reset_that_may_have_lapsed_is_not_offered() {
    for (expires, offers) in [("2026-10-01T11:00:00Z", 0), ("2026-10-03T00:00:00Z", 1)] {
        let lapsing = |observed_at| {
            let mut card = card("acct", 95.0, RENEWS, observed_at, 1);
            card.reading.reset_credits.as_mut().unwrap().next_expires_at = Some(expires.into());
            card
        };
        let first = lapsing("2026-10-01T11:50:00Z");
        let second = lapsing("2026-10-01T12:00:00Z");

        assert_eq!(
            two_polls(&first, &second, &opted_in("acct")),
            offers,
            "{expires}"
        );
    }
}

#[test]
fn an_alerts_lower_share_is_the_one_kept() {
    let alerts = alerts_for("acct", alert(3, false));
    for (used, offers) in [(96.0, 0), (97.0, 1)] {
        let first = card("acct", used, RENEWS, "2026-10-01T11:50:00Z", 1);
        let second = card("acct", used, RENEWS, "2026-10-01T12:00:00Z", 1);

        assert_eq!(two_polls(&first, &second, &alerts), offers, "{used}% used");
    }
}

#[test]
fn exactly_the_wait_before_renewal_is_offered() {
    let a_day_away = "2026-10-02T12:00:00Z";
    let first = card("acct", 95.0, a_day_away, "2026-10-01T11:50:00Z", 1);
    let second = card("acct", 95.0, a_day_away, "2026-10-01T12:00:00Z", 1);

    assert_eq!(two_polls(&first, &second, &opted_in("acct")), 1);
}

#[test]
fn the_fullest_main_window_decides_and_a_models_own_limit_never_does() {
    for (kind, offers) in [(LimitWindowKind::Session, 1), (LimitWindowKind::Model, 0)] {
        let with_window = |observed_at: &str| {
            let mut card = card("acct", 40.0, RENEWS, observed_at, 1);
            card.reading.windows.push(LimitWindowDto {
                id: "other".into(),
                label: "Other".into(),
                kind,
                used_percent: 96.0,
                resets_at: Some("2026-10-01T15:00:00Z".into()),
                window_seconds: Some(18_000),
                observed_at: observed_at.into(),
            });
            card
        };
        let first = with_window("2026-10-01T11:50:00Z");
        let second = with_window("2026-10-01T12:00:00Z");

        assert_eq!(
            two_polls(&first, &second, &opted_in("acct")),
            offers,
            "{kind:?}"
        );
    }
}

#[test]
fn an_account_turned_off_is_forgotten() {
    let mut memory = AlertMemory::default();
    let low = card("acct", 95.0, RENEWS, "2026-10-01T11:50:00Z", 1);
    offered(&mut memory, &low, &opted_in("acct"));
    assert!(!memory.is_empty());

    offered(&mut memory, &low, &HashMap::new());

    assert!(memory.is_empty());
}

#[test]
fn the_notification_names_the_account_what_is_left_and_when_it_renews() {
    let offer = Offer {
        account_id: "acct".into(),
        account_label: Some("you@example.com".into()),
        left_percent: 6.0,
        renews_at: at("2026-10-05T15:00:00Z"),
        automatic: false,
    };

    assert_eq!(
        offer.notice(now()),
        (
            "Codex: a banked reset is available".into(),
            "you@example.com has 6% of its limit left and renews in 4d 3h. Open on-n-off to use a banked reset.".into()
        )
    );
}

fn offers_over(polls: &[(&str, &str)]) -> Vec<usize> {
    let alerts = opted_in("acct");
    let mut memory = AlertMemory::default();
    polls
        .iter()
        .map(|(renews, observed_at)| {
            offered(
                &mut memory,
                &card("acct", 96.0, renews, observed_at, 1),
                &alerts,
            )
        })
        .collect()
}

#[test]
fn a_reset_time_read_a_second_or_two_apart_is_the_same_weekly_cycle() {
    let offers = offers_over(&[
        ("2026-10-05T12:00:00Z", "2026-10-01T11:10:00Z"),
        ("2026-10-05T12:00:01Z", "2026-10-01T11:20:00Z"),
        ("2026-10-05T12:00:00Z", "2026-10-01T11:30:00Z"),
        ("2026-10-05T12:00:00Z", "2026-10-01T11:40:00Z"),
        ("2026-10-05T12:00:01Z", "2026-10-01T11:50:00Z"),
        ("2026-10-05T12:00:01Z", "2026-10-01T12:00:00Z"),
        ("2026-10-05T12:00:02Z", "2026-10-01T12:10:00Z"),
        ("2026-10-05T11:59:59Z", "2026-10-01T12:20:00Z"),
    ]);

    assert_eq!(offers, [0, 1, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn one_reset_time_written_two_ways_is_one_weekly_cycle() {
    let offers = offers_over(&[
        ("2026-10-05T12:00:00+00:00", "2026-10-01T11:50:00Z"),
        ("2026-10-05T12:00:00Z", "2026-10-01T12:00:00Z"),
        ("2026-10-05T12:00:00+00:00", "2026-10-01T12:10:00Z"),
    ]);

    assert_eq!(offers, [0, 1, 0]);
}

#[test]
fn a_reset_time_more_than_two_seconds_away_is_another_weekly_cycle() {
    let offers = offers_over(&[
        ("2026-10-05T12:00:00Z", "2026-10-01T11:30:00Z"),
        ("2026-10-05T12:00:00Z", "2026-10-01T11:40:00Z"),
        ("2026-10-05T12:00:03Z", "2026-10-01T11:50:00Z"),
        ("2026-10-05T12:00:03Z", "2026-10-01T12:00:00Z"),
    ]);

    assert_eq!(offers, [0, 1, 0, 1]);
}

#[test]
fn a_saved_cycle_that_does_not_read_as_a_time_is_another_cycle() {
    let mut memory: AlertMemory = serde_json::from_value(serde_json::json!({
        "acct": {
            "low": {"cycle": "next week", "observedAt": "2026-10-01T11:40:00Z"},
            "offeredCycle": "next week"
        }
    }))
    .unwrap();
    let alerts = opted_in("acct");

    let offers: Vec<usize> = ["2026-10-01T11:50:00Z", "2026-10-01T12:00:00Z"]
        .into_iter()
        .map(|observed_at| {
            offered(
                &mut memory,
                &card("acct", 96.0, RENEWS, observed_at, 1),
                &alerts,
            )
        })
        .collect();

    assert_eq!(offers, [0, 1]);
}
