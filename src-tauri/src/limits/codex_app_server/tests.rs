use super::*;
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
    let mut transport =
        ProcessTransport::spawn_command(&mut command, Duration::from_secs(10), 64).unwrap();

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
        ProcessTransport::spawn_command(&mut command, Duration::from_secs(10), 1024).unwrap();

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
        ProcessTransport::spawn_command(&mut command, Duration::from_secs(10), 1024).unwrap();

    let error = query_app_server(&root, false, &mut transport).unwrap_err();
    let status = transport.finish();
    let failure = classify_query_failure(error, status);

    assert!(matches!(
        failure,
        AppServerFailure::Failed(message)
            if message.contains("may not support `codex app-server`")
                && message.contains("update Codex CLI")
    ));
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

    let failure = classify_query_failure(error, Some(status));

    assert!(matches!(
        failure,
        AppServerFailure::Failed(message)
            if message.contains("may not support `codex app-server`")
                && message.contains("update Codex CLI")
    ));
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

        let failure = classify_query_failure(error, Some(status));

        assert_eq!(
            failure,
            AppServerFailure::Failed(format!("Codex app-server {expected}."))
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

#[test]
fn normalizes_the_chatgpt_account_and_rate_limits_without_reading_a_token() {
    let codex_home = crate::paths::scratch_dir("codex-app-server-normalize");
    std::fs::write(
        codex_home.join("auth.json"),
        r#"{"tokens":{"account_id":"acct-1"}}"#,
    )
    .unwrap();
    let parsed = normalize_app_server(AppServerResult {
        codex_home,
        account: typed(json!({
            "account": {
                "type": "chatgpt",
                "email": "Me@Example.com",
                "planType": "pro"
            },
            "requiresOpenaiAuth": true
        })),
        rate_limits: typed(json!({
            "rateLimits": {
                "limitId": "codex",
                "primary": {
                    "usedPercent": 42,
                    "windowDurationMins": 10080,
                    "resetsAt": 1787838960
                },
                "secondary": null,
                "credits": {"hasCredits": false, "unlimited": false, "balance": "0"},
                "planType": "pro"
            }
        })),
    })
    .unwrap();

    assert_eq!(
        parsed.account,
        Some(crate::dto::LimitsAccountDto {
            id: "acct-1".to_string(),
            label: Some("Me@Example.com".to_string()),
        })
    );
    assert_eq!(parsed.plan.as_deref(), Some("pro"));
    assert_eq!(parsed.windows.len(), 1);
    assert_eq!(parsed.windows[0].used_percent, 42.0);
}

#[test]
fn falls_back_to_a_normalized_email_identity_when_codex_has_no_account_id() {
    let codex_home = crate::paths::scratch_dir("codex-app-server-email");
    let parsed = normalize_app_server(AppServerResult {
        codex_home,
        account: typed(json!({
            "account": {
                "type": "chatgpt",
                "email": " Me@Example.com ",
                "planType": "plus"
            },
            "requiresOpenaiAuth": true
        })),
        rate_limits: typed(json!({"rateLimits": {"planType": "plus"}})),
    })
    .unwrap();

    assert_eq!(
        parsed.account,
        Some(crate::dto::LimitsAccountDto {
            id: "email:me@example.com".to_string(),
            label: Some("Me@Example.com".to_string()),
        })
    );
}

#[test]
fn api_key_accounts_are_explicitly_unsupported() {
    let error = normalize_app_server(AppServerResult {
        codex_home: crate::paths::scratch_dir("codex-app-server-api-key"),
        account: typed(json!({
            "account": {"type": "apiKey"},
            "requiresOpenaiAuth": false
        })),
        rate_limits: typed(json!({"rateLimits": {}})),
    })
    .unwrap_err();

    assert!(matches!(
        error,
        AppServerFailure::Unsupported(message) if message.contains("API key")
    ));
}
