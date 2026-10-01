use super::*;

#[test]
fn a_reset_used_by_hand_is_spent_without_reading_the_limit() {
    let (outcome, transport) = spend_as(
        "acct-1",
        ResetSpend::ByHand,
        |_| signed_in_as("acct-1"),
        consume_transport_at(
            chatgpt_account(),
            40.0,
            Some(json!({"id": 4, "result": {"outcome": "reset"}})),
        ),
    );

    assert_eq!(outcome, Ok(ResetCreditOutcome::Reset));
    let methods: Vec<&Value> = transport
        .as_ref()
        .unwrap()
        .sent
        .iter()
        .map(|message| &message["method"])
        .collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "initialized",
            "account/read",
            "account/rateLimitResetCredit/consume"
        ]
    );
}

#[test]
fn a_reset_used_by_hand_is_spent_where_the_limit_cannot_be_read() {
    let mut transport = consume_transport(chatgpt_account(), None);
    transport.received.pop_back();
    transport
        .received
        .push_back(json!({"id": 3, "error": {"code": -32601, "message": "Method not found"}}));
    transport
        .received
        .push_back(json!({"id": 4, "result": {"outcome": "reset"}}));

    let (outcome, transport) = spend_as(
        "acct-1",
        ResetSpend::ByHand,
        |_| signed_in_as("acct-1"),
        transport,
    );

    assert_eq!(outcome, Ok(ResetCreditOutcome::Reset));
    assert_eq!(consume_requests(&transport.unwrap()).len(), 1);
}

#[test]
fn a_reset_is_spent_automatically_when_no_more_than_the_allowed_share_is_left() {
    for used in [95.0, 97.0, 100.0] {
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
fn a_reset_is_never_spent_automatically_while_more_than_the_allowed_share_is_left() {
    let (outcome, transport) = spend(
        "acct-1",
        |_| signed_in_as("acct-1"),
        consume_transport_at(
            chatgpt_account(),
            94.0,
            Some(json!({"id": 4, "result": {"outcome": "reset"}})),
        ),
    );

    assert_eq!(
        outcome.unwrap_err(),
        "A banked reset can be used once 5% or less of the current limit is left, and 6% is left."
    );
    assert!(consume_requests(&transport.unwrap()).is_empty());
}

#[test]
fn an_accounts_lower_share_is_the_one_kept() {
    let (outcome, transport) = spend_as(
        "acct-1",
        ResetSpend::Automatic {
            max_left_percent: 3,
        },
        |_| signed_in_as("acct-1"),
        consume_transport_at(
            chatgpt_account(),
            96.0,
            Some(json!({"id": 4, "result": {"outcome": "reset"}})),
        ),
    );

    assert_eq!(
        outcome.unwrap_err(),
        "A banked reset can be used once 3% or less of the current limit is left, and 4% is left."
    );
    assert!(consume_requests(&transport.unwrap()).is_empty());
}

#[test]
fn the_fullest_main_window_decides() {
    let (outcome, _) = spend(
        "acct-1",
        |_| signed_in_as("acct-1"),
        consume_transport_reading(
            chatgpt_account(),
            json!({"rateLimits": {
                "limitId": "codex",
                "primary": {"usedPercent": 96.0, "windowDurationMins": 300},
                "secondary": {"usedPercent": 40.0, "windowDurationMins": 10080}
            }}),
            Some(json!({"id": 4, "result": {"outcome": "reset"}})),
        ),
    );

    assert_eq!(outcome, Ok(ResetCreditOutcome::Reset));
}

#[test]
fn one_models_own_limit_never_decides() {
    let (outcome, transport) = spend(
        "acct-1",
        |_| signed_in_as("acct-1"),
        consume_transport_reading(
            chatgpt_account(),
            json!({
                "rateLimits": {
                    "limitId": "codex",
                    "primary": {"usedPercent": 40.0, "windowDurationMins": 10080}
                },
                "rateLimitsByLimitId": {
                    "codex": {"limitId": "codex", "primary": {"usedPercent": 40.0, "windowDurationMins": 10080}},
                    "codex_fable": {"limitId": "codex_fable", "limitName": "Fable", "primary": {"usedPercent": 99.0, "windowDurationMins": 10080}}
                }
            }),
            Some(json!({"id": 4, "result": {"outcome": "reset"}})),
        ),
    );

    assert_eq!(
        outcome.unwrap_err(),
        "A banked reset can be used once 5% or less of the current limit is left, and 60% is left."
    );
    assert!(consume_requests(&transport.unwrap()).is_empty());
}

#[test]
fn a_reset_is_never_spent_automatically_when_how_much_is_left_cannot_be_read() {
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
