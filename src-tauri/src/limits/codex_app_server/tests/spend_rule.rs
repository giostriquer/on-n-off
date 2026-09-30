//! The rule Codex's own app follows for a banked reset, which on-n-off keeps too: one is spent only
//! while the current limit has the allowed share or less left, as read in the same app-server
//! session just before the spend.
use super::*;

#[test]
fn a_reset_is_spent_when_no_more_than_the_allowed_share_is_left() {
    // 90% used is exactly 10% left.
    for used in [90.0, 97.0, 100.0] {
        let (outcome, transport) = spend(
            "acct-1",
            |_| signed_in_as("acct-1"),
            consume_transport_at(
                chatgpt_account(),
                used,
                Some(json!({"id": 4, "result": {"outcome": "reset"}})),
            ),
        );

        assert_eq!(outcome, Ok(ResetCreditOutcome::Reset), "{used}");
        assert_eq!(consume_requests(&transport.unwrap()).len(), 1, "{used}");
    }
}

#[test]
fn a_reset_is_never_spent_while_more_than_the_allowed_share_is_left() {
    let (outcome, transport) = spend(
        "acct-1",
        |_| signed_in_as("acct-1"),
        consume_transport_at(
            chatgpt_account(),
            85.0,
            Some(json!({"id": 4, "result": {"outcome": "reset"}})),
        ),
    );

    assert_eq!(
        outcome.unwrap_err(),
        "A banked reset can be used once 10% or less of the current limit is left, and 15% is left."
    );
    assert!(consume_requests(&transport.unwrap()).is_empty());
}

#[test]
fn an_accounts_lower_share_is_the_one_kept() {
    let (outcome, transport) = spend_within(
        "acct-1",
        5,
        |_| signed_in_as("acct-1"),
        consume_transport_at(
            chatgpt_account(),
            92.0,
            Some(json!({"id": 4, "result": {"outcome": "reset"}})),
        ),
    );

    assert_eq!(
        outcome.unwrap_err(),
        "A banked reset can be used once 5% or less of the current limit is left, and 8% is left."
    );
    assert!(consume_requests(&transport.unwrap()).is_empty());
}

/// Without a limit to read there is no telling how much is left, so no reset is spent.
#[test]
fn a_reset_is_never_spent_when_how_much_is_left_cannot_be_read() {
    let mut transport = consume_transport(chatgpt_account(), None);
    transport.received.pop_back();
    transport
        .received
        .push_back(json!({"id": 3, "result": {"rateLimits": {}}}));
    transport
        .received
        .push_back(json!({"id": 4, "result": {"outcome": "reset"}}));

    let (outcome, transport) = spend("acct-1", |_| signed_in_as("acct-1"), transport);

    assert_eq!(
        outcome.unwrap_err(),
        "on-n-off can't tell how much of the Codex limit is left, so it won't spend a reset."
    );
    assert!(consume_requests(&transport.unwrap()).is_empty());
}
