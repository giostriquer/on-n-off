//! When a banked reset is offered: an opted-in, signed-in Codex account found low on two polls in a
//! row, far enough from its own renewal, with a reset to spend, once per weekly cycle.
use super::*;
use crate::dto::{LimitWindowDto, LimitsResetCreditsDto, Reading};

const NOW: &str = "2026-10-01T12:00:00Z";
/// Four days after [`NOW`], the weekly cycle's end.
const RENEWS: &str = "2026-10-05T12:00:00Z";

fn now() -> DateTime<Utc> {
    instant(NOW).unwrap()
}

/// `account`'s signed-in Codex card, `used` of its weekly window used as observed at `observed_at`,
/// renewing at `renews`, with `banked` resets.
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
                id: "weekly".into(),
                label: "Weekly".into(),
                kind: LimitWindowKind::Weekly,
                used_percent: used,
                resets_at: Some(renews.into()),
                window_seconds: Some(604_800),
                observed_at: observed_at.into(),
            }],
            reset_credits: Some(LimitsResetCreditsDto {
                available_count: banked,
                next_expires_at: None,
                resets: Vec::new(),
            }),
            ..Reading::default()
        })
}

/// The accounts opted in: `account`, at Codex's 10% and a day's wait.
fn opted_in(account: &str) -> HashMap<String, ResetAlert> {
    HashMap::from([(
        account.to_string(),
        ResetAlert {
            label: None,
            max_left_percent: 10,
            min_hours_to_renewal: 24,
        },
    )])
}

/// The offers two polls in a row make, the first reading `first` and the second `second`.
fn two_polls(
    first: &ProviderLimitsDto,
    second: &ProviderLimitsDto,
    alerts: &HashMap<String, ResetAlert>,
) -> (Vec<Offer>, Vec<Offer>) {
    let mut state = HashMap::new();
    let before = observe(&mut state, std::slice::from_ref(first), alerts, now());
    let after = observe(&mut state, std::slice::from_ref(second), alerts, now());
    (before, after)
}

#[test]
fn an_account_found_low_on_two_polls_in_a_row_is_offered_its_reset_once() {
    let alerts = opted_in("acct");
    let first = card("acct", 93.0, RENEWS, "2026-10-01T11:50:00Z", 2);
    let second = card("acct", 94.0, RENEWS, "2026-10-01T12:00:00Z", 2);
    let mut state = HashMap::new();

    let one = observe(&mut state, std::slice::from_ref(&first), &alerts, now());
    let two = observe(&mut state, std::slice::from_ref(&second), &alerts, now());
    let third = card("acct", 95.0, RENEWS, "2026-10-01T12:10:00Z", 2);
    let three = observe(&mut state, std::slice::from_ref(&third), &alerts, now());

    assert!(one.is_empty(), "offered on one reading");
    assert_eq!(
        two,
        [Offer {
            account_label: Some("you@example.com".into()),
            left_percent: 6.0,
            renews_at: instant(RENEWS).unwrap(),
            available: 2,
        }]
    );
    assert!(three.is_empty(), "offered twice in one weekly cycle");
}

#[test]
fn the_same_reading_twice_is_not_two_polls() {
    let alerts = opted_in("acct");
    let low = card("acct", 95.0, RENEWS, "2026-10-01T11:50:00Z", 1);

    let (_, second) = two_polls(&low, &low, &alerts);

    assert!(second.is_empty());
}

#[test]
fn no_reset_is_offered_while_more_than_the_share_is_left() {
    let alerts = opted_in("acct");
    let first = card("acct", 85.0, RENEWS, "2026-10-01T11:50:00Z", 1);
    let second = card("acct", 89.0, RENEWS, "2026-10-01T12:00:00Z", 1);

    assert!(two_polls(&first, &second, &alerts).1.is_empty());
}

/// At exactly the share left, the reset is offered, as the spend itself allows it.
#[test]
fn exactly_the_share_left_is_low() {
    let alerts = opted_in("acct");
    let first = card("acct", 90.0, RENEWS, "2026-10-01T11:50:00Z", 1);
    let second = card("acct", 90.0, RENEWS, "2026-10-01T12:00:00Z", 1);

    assert_eq!(two_polls(&first, &second, &alerts).1.len(), 1);
}

/// A reset spent just before the limit renews by itself is wasted.
#[test]
fn no_reset_is_offered_when_the_limit_renews_soon_anyway() {
    let alerts = opted_in("acct");
    let soon = "2026-10-02T11:00:00Z";
    let first = card("acct", 99.0, soon, "2026-10-01T11:50:00Z", 1);
    let second = card("acct", 99.0, soon, "2026-10-01T12:00:00Z", 1);

    assert!(two_polls(&first, &second, &alerts).1.is_empty());
}

#[test]
fn no_reset_is_offered_without_one_to_spend() {
    let alerts = opted_in("acct");
    let first = card("acct", 99.0, RENEWS, "2026-10-01T11:50:00Z", 0);
    let second = card("acct", 99.0, RENEWS, "2026-10-01T12:00:00Z", 0);

    assert!(two_polls(&first, &second, &alerts).1.is_empty());
}

#[test]
fn an_account_not_opted_in_is_never_offered_a_reset() {
    let alerts = opted_in("someone-else");
    let first = card("acct", 99.0, RENEWS, "2026-10-01T11:50:00Z", 1);
    let second = card("acct", 99.0, RENEWS, "2026-10-01T12:00:00Z", 1);

    assert!(two_polls(&first, &second, &alerts).1.is_empty());
}

/// Only the signed-in account's live read counts: a reset lands on whoever is signed in, and a card
/// that failed or is remembered says nothing new about now.
#[test]
fn only_the_signed_in_accounts_live_read_counts() {
    let alerts = opted_in("acct");
    for change in [
        |card: &mut ProviderLimitsDto| card.current_account = false,
        |card: &mut ProviderLimitsDto| card.status = LimitsStatus::Failed,
        |card: &mut ProviderLimitsDto| card.provider = AgentId::Claude,
    ] {
        let mut first = card("acct", 99.0, RENEWS, "2026-10-01T11:50:00Z", 1);
        let mut second = card("acct", 99.0, RENEWS, "2026-10-01T12:00:00Z", 1);
        change(&mut first);
        change(&mut second);

        assert!(two_polls(&first, &second, &alerts).1.is_empty());
    }
}

/// A poll in between that finds the account no longer low starts the count again.
#[test]
fn a_poll_that_is_not_low_starts_the_count_again() {
    let alerts = opted_in("acct");
    let mut state = HashMap::new();
    for (used, observed_at) in [
        (95.0, "2026-10-01T11:40:00Z"),
        (50.0, "2026-10-01T11:50:00Z"),
        (95.0, "2026-10-01T12:00:00Z"),
    ] {
        let offers = observe(
            &mut state,
            &[card("acct", used, RENEWS, observed_at, 1)],
            &alerts,
            now(),
        );
        assert!(offers.is_empty(), "{used} at {observed_at}");
    }
}

/// A spent reset starts a new weekly cycle, which may be offered its own reset.
#[test]
fn a_new_weekly_cycle_can_be_offered_again() {
    let alerts = opted_in("acct");
    let later = "2026-10-08T12:00:00Z";
    let mut state = HashMap::new();
    let mut offered = 0;
    for (renews, observed_at) in [
        (RENEWS, "2026-10-01T11:40:00Z"),
        (RENEWS, "2026-10-01T11:50:00Z"),
        (later, "2026-10-01T12:00:00Z"),
        (later, "2026-10-01T12:10:00Z"),
    ] {
        offered += observe(
            &mut state,
            &[card("acct", 95.0, renews, observed_at, 1)],
            &alerts,
            now(),
        )
        .len();
    }

    assert_eq!(offered, 2);
}

#[test]
fn an_account_turned_off_is_forgotten() {
    let mut state = HashMap::new();
    let low = card("acct", 95.0, RENEWS, "2026-10-01T11:50:00Z", 1);
    observe(
        &mut state,
        std::slice::from_ref(&low),
        &opted_in("acct"),
        now(),
    );
    assert!(state.contains_key("acct"));

    observe(&mut state, &[low], &HashMap::new(), now());

    assert!(state.is_empty());
}

#[test]
fn the_notification_names_the_account_what_is_left_and_when_it_renews() {
    let offer = Offer {
        account_label: Some("you@example.com".into()),
        left_percent: 6.0,
        renews_at: instant("2026-10-05T15:00:00Z").unwrap(),
        available: 2,
    };

    assert_eq!(
        notification_copy(&offer, now()),
        (
            "Codex: a banked reset is available".into(),
            "you@example.com has 6% of its limit left and renews in 4d 3h. Open on-n-off to use a banked reset.".into()
        )
    );
}
