mod messages;
mod normalize;
mod spend_rule;

use super::*;
use crate::json_line_session::{Exchange, Script};
use serde::de::DeserializeOwned;
use serde_json::json;

fn typed<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn query(script: &Script, codex_home: &Path, force: bool) -> Result<AppServerResult, String> {
    script
        .session(APP_SERVER)
        .run(|session| query_app_server(codex_home, force, session))
}

#[test]
fn completes_the_handshake_and_matches_account_and_rate_limit_responses_by_id() {
    let codex_home = PathBuf::from("/fixture/.codex");
    let account = json!({
        "account": {"type": "chatgpt", "email": "me@example.com", "planType": "pro"},
        "requiresOpenaiAuth": true
    });
    let rate_limits = json!({
        "rateLimits": {
            "limitId": "codex",
            "primary": {"usedPercent": 42, "windowDurationMins": 10080},
            "planType": "pro"
        }
    });
    let script = Script::new([
        json!({"method": "remoteControl/status/changed", "params": {"status": "disabled"}}),
        json!({"id": 1, "result": {
            "userAgent": "on_n_off/0.148.0",
            "codexHome": "/fixture/.codex",
            "platformFamily": "unix",
            "platformOs": "macos"
        }}),
        json!({"id": 2, "result": account.clone()}),
        json!({"id": 3, "result": rate_limits.clone()}),
    ]);

    let result = query(&script, &codex_home, true).unwrap();

    assert_eq!(result.codex_home, codex_home);
    assert_eq!(result.account, typed(account));
    assert_eq!(result.rate_limits, typed(rate_limits));
    assert_eq!(
        script.sent(),
        [
            json!({"id": 1, "method": "initialize", "params": {"clientInfo": {
                "name": "on_n_off", "title": "on-n-off", "version": env!("CARGO_PKG_VERSION")
            }}}),
            json!({"method": "initialized", "params": {}}),
            json!({"id": 2, "method": "account/read", "params": {"refreshToken": true}}),
            json!({"id": 3, "method": "account/rateLimits/read", "params": {}}),
        ]
    );
}

#[test]
fn ordinary_reads_leave_forced_refresh_disabled() {
    let codex_home = PathBuf::from("/fixture/.codex");
    let script = Script::new([
        json!({"id": 1, "result": {
            "userAgent": "on_n_off/0.148.0",
            "codexHome": "/fixture/.codex",
            "platformFamily": "unix",
            "platformOs": "macos"
        }}),
        json!({"id": 2, "result": {
            "account": {"type": "chatgpt", "email": "me@example.com", "planType": "pro"},
            "requiresOpenaiAuth": true
        }}),
        json!({"id": 3, "result": {"rateLimits": {}}}),
    ]);

    query(&script, &codex_home, false).unwrap();

    assert_eq!(script.sent()[2]["params"]["refreshToken"], false);
}

#[test]
fn rate_limits_wait_for_account_refresh_to_complete() {
    let codex_home = PathBuf::from("/fixture/.codex");
    let script = Script::new([
        json!({"id": 1, "result": {
            "userAgent": "on_n_off/0.148.0",
            "codexHome": "/fixture/.codex",
            "platformFamily": "unix",
            "platformOs": "macos"
        }}),
        json!({"id": 2, "result": {
            "account": {"type": "chatgpt", "email": "me@example.com", "planType": "pro"},
            "requiresOpenaiAuth": true
        }}),
        json!({"id": 3, "result": {"rateLimits": {}}}),
    ]);

    let result = query(&script, &codex_home, true);

    assert!(result.is_ok(), "{result:?}");
    let log = script.log();
    let account_answered = log
        .iter()
        .position(|exchange| matches!(exchange, Exchange::Received(message) if message["id"] == 2))
        .unwrap();
    let rate_limits_asked = log
        .iter()
        .position(|exchange| {
            matches!(exchange, Exchange::Sent(message) if message["method"] == "account/rateLimits/read")
        })
        .unwrap();
    assert!(account_answered < rate_limits_asked, "{log:?}");
}

#[test]
fn protocol_errors_are_returned_instead_of_becoming_empty_limits() {
    let codex_home = PathBuf::from("/fixture/.codex");
    let script = Script::new([
        json!({"id": 1, "result": {
            "userAgent": "on_n_off/0.120.0",
            "codexHome": "/fixture/.codex",
            "platformFamily": "unix",
            "platformOs": "macos"
        }}),
        json!({"id": 2, "result": {
            "account": {"type": "chatgpt", "email": "me@example.com", "planType": "pro"},
            "requiresOpenaiAuth": true
        }}),
        json!({"id": 3, "error": {"code": -32601, "message": "Method not found"}}),
    ]);

    let error = query(&script, &codex_home, false).unwrap_err();

    assert!(error.contains("account/rateLimits/read"), "{error}");
    assert!(error.contains("Update Codex CLI"), "{error}");
    assert!(error.contains("Method not found"), "{error}");
}

#[test]
fn malformed_rate_limit_payload_is_a_failure_instead_of_an_empty_success() {
    let codex_home = PathBuf::from("/fixture/.codex");
    let script = Script::new([
        json!({"id": 1, "result": {"codexHome": "/fixture/.codex"}}),
        json!({"id": 2, "result": {
            "account": {"type": "chatgpt", "email": "me@example.com", "planType": "pro"}
        }}),
        json!({"id": 3, "result": {"unexpected": true}}),
    ]);

    let error = query(&script, &codex_home, false).unwrap_err();

    assert!(error.contains("rateLimits"), "{error}");
    assert!(error.contains("malformed response"), "{error}");
}

#[cfg(unix)]
#[test]
fn accepts_a_symlinked_codex_home_that_resolves_to_the_expected_directory() {
    use std::os::unix::fs::symlink;

    let root = crate::paths::scratch_dir("codex-app-server-home-link");
    let expected = root.join("real");
    let alias = root.join("alias");
    std::fs::create_dir_all(&expected).unwrap();
    symlink(&expected, &alias).unwrap();
    let script = Script::new([
        json!({"id": 1, "result": {
            "userAgent": "on_n_off/0.148.0",
            "codexHome": alias,
            "platformFamily": "unix",
            "platformOs": "macos"
        }}),
        json!({"id": 2, "result": {"account": null, "requiresOpenaiAuth": true}}),
        json!({"id": 3, "result": {"rateLimits": {}}}),
    ]);

    let result = query(&script, &expected, false);

    assert!(result.is_ok(), "{result:?}");
}

fn consume_replies(account: Value, consume_reply: Option<Value>) -> Vec<Value> {
    consume_replies_at(account, 95.0, consume_reply)
}

fn consume_replies_at(
    account: Value,
    used_percent: f64,
    consume_reply: Option<Value>,
) -> Vec<Value> {
    consume_replies_reading(
        account,
        json!({"rateLimits": {
            "limitId": "codex",
            "primary": {"usedPercent": used_percent, "windowDurationMins": 10080}
        }}),
        consume_reply,
    )
}

fn consume_replies_reading(
    account: Value,
    rate_limits: Value,
    consume_reply: Option<Value>,
) -> Vec<Value> {
    let mut replies = vec![
        json!({"id": 1, "result": {
            "userAgent": "on_n_off/0.154.0",
            "codexHome": "/fixture/.codex",
            "platformFamily": "unix",
            "platformOs": "macos"
        }}),
        json!({"id": 2, "result": account}),
        json!({"id": 3, "result": rate_limits}),
    ];
    replies.extend(consume_reply);
    replies
}

fn chatgpt_account() -> Value {
    json!({"account": {"type": "chatgpt", "email": "me@example.com", "planType": "pro"}})
}

fn signed_in_as(id: &str) -> Result<Option<(String, Value)>, String> {
    Ok(Some((id.to_string(), json!({}))))
}

fn spend(
    card: &str,
    identity: impl Fn(&Path) -> Result<Option<(String, Value)>, String>,
    replies: Vec<Value>,
) -> (Result<ResetCreditOutcome, String>, Option<Script>) {
    spend_as(
        card,
        ResetSpend::Automatic {
            max_left_percent: crate::settings::CODEX_RESET_MAX_LEFT_PERCENT,
        },
        identity,
        replies,
    )
}

fn spend_as(
    card: &str,
    how: ResetSpend,
    identity: impl Fn(&Path) -> Result<Option<(String, Value)>, String>,
    replies: Vec<Value>,
) -> (Result<ResetCreditOutcome, String>, Option<Script>) {
    let codex_home = PathBuf::from("/fixture/.codex");
    let script = Script::new(replies);
    let started = std::cell::Cell::new(false);
    let result = spend_reset_credit(&codex_home, card, "attempt-1", how, identity, |_| {
        started.set(true);
        Ok(script.session(APP_SERVER))
    });
    (result, started.get().then_some(script))
}

fn consume_requests(script: &Script) -> Vec<Value> {
    script
        .sent()
        .into_iter()
        .filter(|message| message["method"] == "account/rateLimitResetCredit/consume")
        .collect()
}

#[test]
fn spending_a_reset_credit_completes_the_handshake_then_redeems_exactly_one() {
    let (outcome, script) = spend(
        "acct-1",
        |_| signed_in_as("acct-1"),
        consume_replies(
            chatgpt_account(),
            Some(json!({"id": 4, "result": {"outcome": "reset"}})),
        ),
    );

    assert_eq!(outcome, Ok(ResetCreditOutcome::Reset));
    assert_eq!(
        script.unwrap().sent(),
        [
            json!({"id": 1, "method": "initialize", "params": {"clientInfo": {
                "name": "on_n_off", "title": "on-n-off", "version": env!("CARGO_PKG_VERSION")
            }}}),
            json!({"method": "initialized", "params": {}}),
            json!({"id": 2, "method": "account/read", "params": {"refreshToken": false}}),
            json!({"id": 3, "method": "account/rateLimits/read", "params": {}}),
            json!({"id": 4, "method": "account/rateLimitResetCredit/consume",
                   "params": {"idempotencyKey": "attempt-1"}}),
        ]
    );
}

#[test]
fn every_consume_outcome_reaches_the_caller_and_a_new_one_is_not_a_failure() {
    for (wire, expected) in [
        ("reset", ResetCreditOutcome::Reset),
        ("nothingToReset", ResetCreditOutcome::NothingToReset),
        ("noCredit", ResetCreditOutcome::NoCredit),
        ("alreadyRedeemed", ResetCreditOutcome::AlreadyRedeemed),
        ("somethingCodexAddedLater", ResetCreditOutcome::Unknown),
    ] {
        let (outcome, _) = spend(
            "acct-1",
            |_| signed_in_as("acct-1"),
            consume_replies(
                chatgpt_account(),
                Some(json!({"id": 4, "result": {"outcome": wire}})),
            ),
        );
        assert_eq!(outcome, Ok(expected), "{wire}");
    }
}

#[test]
fn a_reset_is_not_spent_when_the_signed_in_account_is_not_the_cards() {
    for native in [signed_in_as("acct-2"), Ok(None)] {
        let (outcome, script) = spend(
            "acct-1",
            move |_| native.clone(),
            consume_replies(chatgpt_account(), None),
        );

        assert!(outcome.is_err(), "{outcome:?}");
        assert!(script.is_none());
    }
    let (changed, _) = spend(
        "acct-1",
        |_| signed_in_as("acct-2"),
        consume_replies(chatgpt_account(), None),
    );
    assert!(changed.unwrap_err().contains("changed"));
    let (unknown, _) = spend(
        "acct-1",
        |_| Ok(None),
        consume_replies(chatgpt_account(), None),
    );
    assert!(unknown
        .unwrap_err()
        .contains("can't confirm which Codex account"));
}

#[test]
fn a_login_that_changes_while_the_app_server_starts_is_caught_before_the_request() {
    let checks = std::cell::Cell::new(0);
    let (outcome, script) = spend(
        "acct-1",
        |_| {
            checks.set(checks.get() + 1);
            signed_in_as(if checks.get() == 1 {
                "acct-1"
            } else {
                "acct-2"
            })
        },
        consume_replies(
            chatgpt_account(),
            Some(json!({"id": 4, "result": {"outcome": "reset"}})),
        ),
    );

    assert!(outcome.unwrap_err().contains("changed"));
    assert_eq!(checks.get(), 2);
    assert!(consume_requests(&script.unwrap()).is_empty());
}

#[test]
fn a_reset_credit_is_never_spent_without_a_chatgpt_login() {
    for (account, refusal) in [
        (
            json!({"account": null}),
            "Codex is not signed in, so there is no banked reset to use.",
        ),
        (
            json!({"account": {"type": "apiKey"}}),
            "Banked resets belong to a ChatGPT sign-in, and Codex is not using one.",
        ),
        (
            json!({"account": {"type": "somethingNew"}}),
            "Banked resets belong to a ChatGPT sign-in, and Codex is not using one.",
        ),
    ] {
        let (outcome, script) = spend(
            "acct-1",
            |_| signed_in_as("acct-1"),
            consume_replies(account, None),
        );

        assert_eq!(outcome, Err(refusal.to_string()));
        assert!(consume_requests(&script.unwrap()).is_empty());
    }
}

#[test]
fn a_cli_without_banked_resets_is_asked_to_update() {
    let (outcome, _) = spend(
        "acct-1",
        |_| signed_in_as("acct-1"),
        consume_replies(
            chatgpt_account(),
            Some(json!({"id": 4, "error": {"code": -32601, "message": "Method not found"}})),
        ),
    );

    let error = outcome.unwrap_err();
    assert!(
        error.contains("account/rateLimitResetCredit/consume"),
        "{error}"
    );
    assert!(error.contains("Update Codex CLI"), "{error}");
}

#[test]
fn the_card_account_id_from_a_read_is_the_id_a_spend_accepts() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let codex_home = crate::paths::scratch_dir("codex-app-server-reset-card-id");
    let payload = json!({"https://api.openai.com/auth": {"chatgpt_user_id": "user-1", "chatgpt_account_id": "workspace-1"}});
    let token = format!(
        "header.{}.signature",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap())
    );
    std::fs::write(
        codex_home.join("auth.json"),
        json!({"tokens": {"account_id": "workspace-1", "id_token": token}}).to_string(),
    )
    .unwrap();
    let before = crate::accounts::codex_store::metadata(&codex_home).unwrap();
    let card = normalize_app_server(
        AppServerResult {
            codex_home: codex_home.clone(),
            account: typed(chatgpt_account()),
            rate_limits: typed(json!({"rateLimits": {"planType": "pro"}})),
        },
        before,
    )
    .unwrap()
    .0
    .account
    .unwrap()
    .id;

    assert_eq!(
        reset_target_matches(
            crate::accounts::codex_store::metadata(&codex_home).unwrap(),
            &card
        ),
        Ok(())
    );
}

fn business_session(codex_home: &Path, kind: &str) -> Script {
    Script::new([
        json!({"id": 1, "result": {
            "userAgent": "on_n_off/0.148.0",
            "codexHome": codex_home.to_string_lossy(),
            "platformFamily": "unix",
            "platformOs": "macos"
        }}),
        json!({"id": 2, "result": {
            "account": {"type": kind, "email": "you@example.com", "planType": "self_serve_business_prolite"},
            "requiresOpenaiAuth": true
        }}),
        json!({"id": 3, "result": {"rateLimits": {
            "limitId": "codex",
            "primary": {"usedPercent": 12, "windowDurationMins": 10080},
            "credits": {"hasCredits": true, "unlimited": false, "balance": null},
            "planType": "self_serve_business_prolite"
        }}}),
    ])
}

fn business_home(name: &str) -> PathBuf {
    let home = crate::paths::scratch_dir(name);
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::write(
        home.join(".codex/auth.json"),
        r#"{"tokens":{"account_id":"acct-1","access_token":"fixture-access"}}"#,
    )
    .unwrap();
    home
}

#[test]
fn a_signed_in_read_asks_what_its_account_spent_once_the_account_is_confirmed() {
    let home = business_home("codex-app-server-spent");
    let codex_home = home.join(".codex");
    let spent = crate::dto::LimitsCreditsSpentDto {
        last_7_days: 18303.4,
        last_30_days: 20299.7,
        updated_at: None,
    };
    let asked = std::cell::RefCell::new(Vec::new());

    let parsed = read_with(
        &home,
        false,
        |_| Ok(business_session(&codex_home, "chatgpt").session(APP_SERVER)),
        |parsed: &mut Parsed, access: Option<&CodexAccess>| {
            asked.borrow_mut().push((
                access.map(|access| (access.observation_key.clone(), access.token.authorization())),
                parsed.account.as_ref().map(|account| account.id.clone()),
                parsed.reading.plan.clone(),
            ));
            parsed.reading.credits_spent = Some(spent.clone());
        },
    )
    .unwrap();

    assert_eq!(parsed.reading.credits_spent, Some(spent));
    assert_eq!(
        asked.into_inner(),
        vec![(
            Some(("acct-1".to_string(), "Bearer fixture-access".to_string())),
            Some("acct-1".to_string()),
            Some("self_serve_business_prolite".to_string())
        )]
    );
}

#[test]
fn a_failed_signed_in_read_never_asks_what_it_spent() {
    let home = business_home("codex-app-server-spent-failed");
    let codex_home = home.join(".codex");

    let not_chatgpt = read_with(
        &home,
        false,
        |_| Ok(business_session(&codex_home, "apiKey").session(APP_SERVER)),
        |_: &mut Parsed, _: Option<&CodexAccess>| panic!("asked what an API key spent"),
    );
    let no_process = read_with(
        &home,
        false,
        |_| Err("codex is not installed".to_string()),
        |_: &mut Parsed, _: Option<&CodexAccess>| panic!("asked without a read"),
    );

    assert!(matches!(not_chatgpt, Err(AppServerFailure::Unsupported(_))));
    assert!(matches!(no_process, Err(AppServerFailure::Failed(_))));
}
