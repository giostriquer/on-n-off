mod normalize;

use super::*;
use crate::cli_stub::ANSWER_DEADLINE;
use serde_json::json;
use std::collections::VecDeque;

fn typed<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

struct FakeTransport {
    received: VecDeque<Value>,
    sent: Vec<Value>,
}

struct RefreshOrderingTransport {
    received: VecDeque<Value>,
    account_response_consumed: bool,
}

impl JsonLineTransport for RefreshOrderingTransport {
    fn send(&mut self, message: &Value) -> Result<(), TransportError> {
        if message.get("method").and_then(Value::as_str) == Some("account/rateLimits/read")
            && !self.account_response_consumed
        {
            return Err(TransportError {
                kind: QueryErrorKind::Other,
                message: "rate limits requested before account refresh completed".to_string(),
            });
        }
        Ok(())
    }

    fn receive(&mut self) -> Result<Value, TransportError> {
        let message = self.received.pop_front().ok_or_else(|| TransportError {
            kind: QueryErrorKind::Other,
            message: "no more app-server messages".to_string(),
        })?;
        if message.get("id").and_then(Value::as_u64) == Some(2) {
            self.account_response_consumed = true;
        }
        Ok(message)
    }
}

impl JsonLineTransport for FakeTransport {
    fn send(&mut self, message: &Value) -> Result<(), TransportError> {
        self.sent.push(message.clone());
        Ok(())
    }

    fn receive(&mut self) -> Result<Value, TransportError> {
        self.received.pop_front().ok_or_else(|| TransportError {
            kind: QueryErrorKind::Other,
            message: "no more app-server messages".to_string(),
        })
    }
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
    let mut transport = FakeTransport {
        received: VecDeque::from([
            json!({"method": "remoteControl/status/changed", "params": {"status": "disabled"}}),
            json!({"id": 1, "result": {
                "userAgent": "on_n_off/0.148.0",
                "codexHome": "/fixture/.codex",
                "platformFamily": "unix",
                "platformOs": "macos"
            }}),
            json!({"id": 2, "result": account.clone()}),
            json!({"id": 3, "result": rate_limits.clone()}),
        ]),
        sent: Vec::new(),
    };

    let result = query_app_server(&codex_home, true, &mut transport).unwrap();

    assert_eq!(result.codex_home, codex_home);
    assert_eq!(result.account, typed(account));
    assert_eq!(result.rate_limits, typed(rate_limits));
    assert_eq!(
        transport.sent,
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
    let mut transport = FakeTransport {
        received: VecDeque::from([
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
        ]),
        sent: Vec::new(),
    };

    query_app_server(&codex_home, false, &mut transport).unwrap();

    assert_eq!(transport.sent[2]["params"]["refreshToken"], false);
}

#[test]
fn rate_limits_wait_for_account_refresh_to_complete() {
    let codex_home = PathBuf::from("/fixture/.codex");
    let mut transport = RefreshOrderingTransport {
        received: VecDeque::from([
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
        ]),
        account_response_consumed: false,
    };

    let result = query_app_server(&codex_home, true, &mut transport);

    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn protocol_errors_are_returned_instead_of_becoming_empty_limits() {
    let codex_home = PathBuf::from("/fixture/.codex");
    let mut transport = FakeTransport {
        received: VecDeque::from([
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
        ]),
        sent: Vec::new(),
    };

    let error = query_app_server(&codex_home, false, &mut transport)
        .unwrap_err()
        .message;

    assert!(error.contains("account/rateLimits/read"), "{error}");
    assert!(error.contains("Update Codex CLI"), "{error}");
    assert!(error.contains("Method not found"), "{error}");
}

#[test]
fn malformed_rate_limit_payload_is_a_failure_instead_of_an_empty_success() {
    let codex_home = PathBuf::from("/fixture/.codex");
    let mut transport = FakeTransport {
        received: VecDeque::from([
            json!({"id": 1, "result": {"codexHome": "/fixture/.codex"}}),
            json!({"id": 2, "result": {
                "account": {"type": "chatgpt", "email": "me@example.com", "planType": "pro"}
            }}),
            json!({"id": 3, "result": {"unexpected": true}}),
        ]),
        sent: Vec::new(),
    };

    let error = query_app_server(&codex_home, false, &mut transport)
        .unwrap_err()
        .message;

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
    let mut transport = FakeTransport {
        received: VecDeque::from([
            json!({"id": 1, "result": {
                "userAgent": "on_n_off/0.148.0",
                "codexHome": alias,
                "platformFamily": "unix",
                "platformOs": "macos"
            }}),
            json!({"id": 2, "result": {"account": null, "requiresOpenaiAuth": true}}),
            json!({"id": 3, "result": {"rateLimits": {}}}),
        ]),
        sent: Vec::new(),
    };

    let result = query_app_server(&expected, false, &mut transport);

    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn process_transport_times_out_and_stops_the_child() {
    let root = crate::paths::scratch_dir("codex-app-server-timeout");
    let cli = crate::cli_stub::CliStub::new("codex").sleep(5).cli(&root);
    let mut command = cli.command();
    let started = Instant::now();
    let mut transport =
        ProcessTransport::spawn_command(&mut command, Duration::from_millis(100), 1024).unwrap();

    let error = transport.receive().unwrap_err().message;
    transport.finish();

    assert!(error.contains("timed out"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(transport.child.try_wait().unwrap().is_some());
}

#[test]
fn process_transport_rejects_an_oversized_stdout_line() {
    let root = crate::paths::scratch_dir("codex-app-server-output-limit");
    let output = "x".repeat(128);
    let cli = crate::cli_stub::CliStub::new("codex")
        .stdout(&output)
        .cli(&root);
    let mut command = cli.command();
    let mut transport = ProcessTransport::spawn_command(&mut command, ANSWER_DEADLINE, 64).unwrap();

    let error = transport.receive().unwrap_err().message;
    transport.finish();

    assert!(error.contains("exceeded 64 bytes"), "{error}");
}

#[test]
fn process_transport_does_not_surface_stderr_content() {
    let root = crate::paths::scratch_dir("codex-app-server-stderr");
    let cli = crate::cli_stub::CliStub::new("codex")
        .stdout("not-json")
        .stderr("sensitive-provider-diagnostic")
        .cli(&root);
    let mut command = cli.command();
    let mut transport =
        ProcessTransport::spawn_command(&mut command, ANSWER_DEADLINE, 1024).unwrap();

    let error = transport.receive().unwrap_err().message;
    transport.finish();

    assert!(error.contains("invalid JSON"), "{error}");
    assert!(!error.contains("sensitive-provider-diagnostic"), "{error}");
}

#[test]
fn early_nonzero_exit_explains_that_the_cli_may_need_an_update() {
    let root = crate::paths::scratch_dir("codex-app-server-old-cli");
    let cli = crate::cli_stub::CliStub::new("codex").exit(2).cli(&root);
    let mut command = cli.command();
    let mut transport =
        ProcessTransport::spawn_command(&mut command, ANSWER_DEADLINE, 1024).unwrap();

    let error = query_app_server(&root, false, &mut transport).unwrap_err();
    let status = transport.finish();
    let message = classify_query_failure(error, status);

    assert!(
        message.contains("may not support `codex app-server`"),
        "{message}"
    );
    assert!(message.contains("update Codex CLI"), "{message}");
}

#[test]
fn early_broken_pipe_and_nonzero_exit_give_the_same_old_cli_guidance() {
    let root = crate::paths::scratch_dir("codex-app-server-old-cli-write");
    let cli = crate::cli_stub::CliStub::new("codex").exit(2).cli(&root);
    let status = cli.command().status().unwrap();
    let error = QueryError {
        stage: QueryStage::Initialize,
        kind: QueryErrorKind::TransportClosed,
        message: "Could not write to Codex app-server: Broken pipe".to_string(),
    };

    let message = classify_query_failure(error, Some(status));

    assert!(
        message.contains("may not support `codex app-server`"),
        "{message}"
    );
    assert!(message.contains("update Codex CLI"), "{message}");
}

#[test]
fn nonzero_exit_does_not_hide_typed_timeout_or_invalid_output_errors() {
    for (kind, expected) in [
        (QueryErrorKind::Timeout, "timed out"),
        (QueryErrorKind::InvalidOutput, "invalid JSON"),
    ] {
        let root = crate::paths::scratch_dir("codex-app-server-specific-error");
        let cli = crate::cli_stub::CliStub::new("codex").exit(2).cli(&root);
        let status = cli.command().status().unwrap();
        let error = QueryError {
            stage: QueryStage::Initialize,
            kind,
            message: format!("Codex app-server {expected}."),
        };

        assert_eq!(
            classify_query_failure(error, Some(status)),
            format!("Codex app-server {expected}.")
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_home_comparison_is_case_and_separator_insensitive() {
    assert!(path_values_are_equivalent(
        Path::new(r"C:\Users\Me\.codex"),
        Path::new("c:/users/me/.codex")
    ));
}

fn consume_transport(account: Value, consume_reply: Option<Value>) -> FakeTransport {
    let mut received = VecDeque::from([
        json!({"id": 1, "result": {
            "userAgent": "on_n_off/0.154.0",
            "codexHome": "/fixture/.codex",
            "platformFamily": "unix",
            "platformOs": "macos"
        }}),
        json!({"id": 2, "result": account}),
    ]);
    received.extend(consume_reply);
    FakeTransport {
        received,
        sent: Vec::new(),
    }
}

fn chatgpt_account() -> Value {
    json!({"account": {"type": "chatgpt", "email": "me@example.com", "planType": "pro"}})
}

fn signed_in_as(id: &str) -> Result<Option<(String, Value)>, String> {
    Ok(Some((id.to_string(), json!({}))))
}

/// Spend through the real checks with a fake app-server; the transport comes back for inspection,
/// or `None` when the checks refused before starting one.
fn spend(
    card: &str,
    identity: impl Fn(&Path) -> Result<Option<(String, Value)>, String>,
    transport: FakeTransport,
) -> (Result<ResetCreditOutcome, String>, Option<FakeTransport>) {
    let codex_home = PathBuf::from("/fixture/.codex");
    let spawned = std::cell::RefCell::new(None);
    struct Recording<'a>(FakeTransport, &'a std::cell::RefCell<Option<FakeTransport>>);
    impl JsonLineTransport for Recording<'_> {
        fn send(&mut self, message: &Value) -> Result<(), TransportError> {
            self.0.send(message)
        }
        fn receive(&mut self) -> Result<Value, TransportError> {
            self.0.receive()
        }
        fn finish(&mut self) -> Option<ExitStatus> {
            *self.1.borrow_mut() = Some(FakeTransport {
                received: std::mem::take(&mut self.0.received),
                sent: std::mem::take(&mut self.0.sent),
            });
            None
        }
    }
    let result = spend_reset_credit(&codex_home, card, "attempt-1", identity, |_| {
        Ok(Recording(transport, &spawned))
    });
    (result, spawned.into_inner())
}

fn consume_requests(transport: &FakeTransport) -> Vec<&Value> {
    transport
        .sent
        .iter()
        .filter(|message| message["method"] == "account/rateLimitResetCredit/consume")
        .collect()
}

#[test]
fn spending_a_reset_credit_completes_the_handshake_then_redeems_exactly_one() {
    let (outcome, transport) = spend(
        "acct-1",
        |_| signed_in_as("acct-1"),
        consume_transport(
            chatgpt_account(),
            Some(json!({"id": 3, "result": {"outcome": "reset"}})),
        ),
    );

    assert_eq!(outcome, Ok(ResetCreditOutcome::Reset));
    assert_eq!(
        transport.unwrap().sent,
        [
            json!({"id": 1, "method": "initialize", "params": {"clientInfo": {
                "name": "on_n_off", "title": "on-n-off", "version": env!("CARGO_PKG_VERSION")
            }}}),
            json!({"method": "initialized", "params": {}}),
            // Spending a reset never forces a token refresh of its own.
            json!({"id": 2, "method": "account/read", "params": {"refreshToken": false}}),
            json!({"id": 3, "method": "account/rateLimitResetCredit/consume",
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
        // The request went through; an outcome this build does not know must not read as an error.
        ("somethingCodexAddedLater", ResetCreditOutcome::Unknown),
    ] {
        let (outcome, _) = spend(
            "acct-1",
            |_| signed_in_as("acct-1"),
            consume_transport(
                chatgpt_account(),
                Some(json!({"id": 3, "result": {"outcome": wire}})),
            ),
        );
        assert_eq!(outcome, Ok(expected), "{wire}");
    }
}

#[test]
fn a_reset_is_not_spent_when_the_signed_in_account_is_not_the_cards() {
    for native in [signed_in_as("acct-2"), Ok(None)] {
        let (outcome, transport) = spend(
            "acct-1",
            move |_| native.clone(),
            consume_transport(chatgpt_account(), None),
        );

        assert!(outcome.is_err(), "{outcome:?}");
        // Refused before an app-server was ever started.
        assert!(transport.is_none());
    }
    let (changed, _) = spend(
        "acct-1",
        |_| signed_in_as("acct-2"),
        consume_transport(chatgpt_account(), None),
    );
    assert!(changed.unwrap_err().contains("changed"));
    let (unknown, _) = spend(
        "acct-1",
        |_| Ok(None),
        consume_transport(chatgpt_account(), None),
    );
    assert!(unknown
        .unwrap_err()
        .contains("can't confirm which Codex account"));
}

#[test]
fn a_login_that_changes_while_the_app_server_starts_is_caught_before_the_request() {
    let checks = std::cell::Cell::new(0);
    let (outcome, transport) = spend(
        "acct-1",
        |_| {
            checks.set(checks.get() + 1);
            // The first check passes; by the time app-server has loaded, another login is active.
            signed_in_as(if checks.get() == 1 {
                "acct-1"
            } else {
                "acct-2"
            })
        },
        consume_transport(
            chatgpt_account(),
            Some(json!({"id": 3, "result": {"outcome": "reset"}})),
        ),
    );

    assert!(outcome.unwrap_err().contains("changed"));
    assert_eq!(checks.get(), 2);
    assert!(consume_requests(&transport.unwrap()).is_empty());
}

#[test]
fn a_reset_credit_is_never_spent_without_a_chatgpt_login() {
    for account in [
        json!({"account": null}),
        json!({"account": {"type": "apiKey"}}),
    ] {
        let (outcome, transport) = spend(
            "acct-1",
            |_| signed_in_as("acct-1"),
            consume_transport(account, None),
        );

        assert!(outcome.is_err(), "{outcome:?}");
        assert!(consume_requests(&transport.unwrap()).is_empty());
    }
}

#[test]
fn a_cli_without_banked_resets_is_asked_to_update() {
    let (outcome, _) = spend(
        "acct-1",
        |_| signed_in_as("acct-1"),
        consume_transport(
            chatgpt_account(),
            Some(json!({"id": 3, "error": {"code": -32601, "message": "Method not found"}})),
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
    let before = crate::accounts::native::codex_metadata(&codex_home).unwrap();
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
            crate::accounts::native::codex_metadata(&codex_home).unwrap(),
            &card
        ),
        Ok(())
    );
}

/// An app-server session for a signed-in business member of `acct-1`, answered in order.
fn business_session(codex_home: &Path, kind: &str) -> FakeTransport {
    FakeTransport {
        received: VecDeque::from([
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
        ]),
        sent: Vec::new(),
    }
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

/// The spending read runs after the identity check, for the account and plan the card was read
/// for, and its figure is on the card before the card is remembered.
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
        |_| Ok(business_session(&codex_home, "chatgpt")),
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

/// The backend reads run with the login's own token, for the card the identity check confirmed,
/// and both figures land on that card: its term, and what it spent as a workspace member.
#[test]
fn a_signed_in_read_takes_its_term_and_spending_with_its_own_token_once_the_account_is_confirmed() {
    let home = business_home("codex-app-server-term");
    let codex_home = home.join(".codex");
    super::super::renewal::forget("acct-1");
    super::super::credits_spent::forget("acct-1");
    let (url, request) = crate::http::serve_once_capturing(
        "200 OK",
        &[],
        r#"{"active_until":"2026-09-28T16:22:34Z","will_renew":false,"cancellation_outcome":"user_cancelled"}"#,
    );
    let today = chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let (breakdown, spending) = crate::http::serve_once_capturing(
        "200 OK",
        &[],
        &json!({"units": "credits", "data": [{"date": today, "models": [{"model": "m", "credits": 5.5}]}]})
            .to_string(),
    );

    let parsed = read_with(
        &home,
        false,
        |_| Ok(business_session(&codex_home, "chatgpt")),
        |parsed: &mut Parsed, access: Option<&CodexAccess>| {
            backend_reads(
                parsed,
                access,
                BackendUrls {
                    credit_usage: &breakdown,
                    subscriptions: &url,
                },
                chrono::Utc::now(),
            );
        },
    )
    .unwrap();
    let head = request.join().unwrap().head;
    let spending_head = spending.join().unwrap().head;

    let term = parsed
        .reading
        .subscription
        .expect("the term is on the card");
    assert!(!term.will_renew);
    assert_eq!(term.note, Some(crate::dto::SubscriptionNote::Cancelled));
    assert!(head.contains("account_id=acct-1"), "{head}");
    assert!(head.contains("Bearer fixture-access"), "{head}");
    assert_eq!(
        parsed.reading.credits_spent.map(|spent| spent.last_7_days),
        Some(5.5),
        "what the member spent is on the card too"
    );
    assert!(
        spending_head.contains("Bearer fixture-access"),
        "{spending_head}"
    );
    super::super::renewal::forget("acct-1");
    super::super::credits_spent::forget("acct-1");
}

/// A read that fails, or a login that is not a ChatGPT one, is never asked what it spent.
#[test]
fn a_failed_signed_in_read_never_asks_what_it_spent() {
    let home = business_home("codex-app-server-spent-failed");
    let codex_home = home.join(".codex");

    let not_chatgpt = read_with(
        &home,
        false,
        |_| Ok(business_session(&codex_home, "apiKey")),
        |_: &mut Parsed, _: Option<&CodexAccess>| panic!("asked what an API key spent"),
    );
    let no_process = read_with(
        &home,
        false,
        |_| Err::<FakeTransport, _>("codex is not installed".to_string()),
        |_: &mut Parsed, _: Option<&CodexAccess>| panic!("asked without a read"),
    );

    assert!(matches!(not_chatgpt, Err(AppServerFailure::Unsupported(_))));
    assert!(matches!(no_process, Err(AppServerFailure::Failed(_))));
}
