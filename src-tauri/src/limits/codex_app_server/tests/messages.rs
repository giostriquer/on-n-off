use super::*;
use crate::cli_stub::{CliStub, ANSWER_DEADLINE};
use std::process::Command;
use std::time::Duration;

fn app_server(stub: CliStub, name: &str, timeout: Duration, line_limit: usize) -> ProcessTransport {
    let root = crate::paths::scratch_dir(name);
    let mut command = stub.cli(&root).command();
    match ProcessTransport::spawn_command(&mut command, timeout, line_limit) {
        Ok(transport) => transport,
        Err(error) => panic!("{error}"),
    }
}

fn read_failure(mut transport: impl JsonLineTransport) -> String {
    let error = query_app_server(Path::new("/fixture/.codex"), false, &mut transport).unwrap_err();
    let status = transport.finish();
    classify_query_failure(error, status)
}

fn scripted(received: Vec<Value>) -> FakeTransport {
    FakeTransport {
        received: received.into(),
        sent: Vec::new(),
    }
}

fn initialized() -> Value {
    json!({"id": 1, "result": {"codexHome": "/fixture/.codex"}})
}

fn chatgpt() -> Value {
    json!({"id": 2, "result": {"account": {"type": "chatgpt", "email": "you@example.com"}}})
}

#[test]
fn a_missing_codex_cli_says_how_to_install_it() {
    let root = crate::paths::scratch_dir("codex-app-server-missing");
    let mut command = Command::new(root.join("codex"));

    let error = ProcessTransport::spawn_command(&mut command, ANSWER_DEADLINE, 1024).err();

    assert_eq!(
        error.as_deref(),
        Some("Codex CLI not found. Install or configure `codex`, then refresh Limits.")
    );
}

#[test]
fn a_codex_app_server_that_never_answers_times_out_by_name() {
    let transport = app_server(
        CliStub::new("codex").sleep(5),
        "codex-app-server-timeout-message",
        Duration::from_millis(100),
        1024,
    );

    assert_eq!(read_failure(transport), "Codex app-server timed out.");
}

#[test]
fn an_oversized_codex_app_server_line_is_named_with_its_limit() {
    let transport = app_server(
        CliStub::new("codex").stdout(&"x".repeat(128)).sleep(1),
        "codex-app-server-line-message",
        ANSWER_DEADLINE,
        64,
    );

    assert_eq!(
        read_failure(transport),
        "Codex app-server output line exceeded 64 bytes."
    );
}

#[test]
fn codex_app_server_output_that_is_not_json_is_named() {
    let transport = app_server(
        CliStub::new("codex").stdout("not-json").sleep(1),
        "codex-app-server-json-message",
        ANSWER_DEADLINE,
        1024,
    );

    let message = read_failure(transport);

    assert!(
        message.starts_with("Codex app-server returned invalid JSON: "),
        "{message}"
    );
}

#[test]
fn a_codex_app_server_that_closes_cleanly_before_answering_says_so() {
    let transport = app_server(
        CliStub::new("codex").sleep(1),
        "codex-app-server-closed-message",
        ANSWER_DEADLINE,
        1024,
    );

    assert_eq!(
        read_failure(transport),
        "Codex app-server closed before returning limits."
    );
}

#[test]
fn a_codex_cli_that_exits_unsuccessfully_before_initializing_is_asked_to_update() {
    let transport = app_server(
        CliStub::new("codex").exit(2),
        "codex-app-server-exit-message",
        ANSWER_DEADLINE,
        1024,
    );

    assert_eq!(
        read_failure(transport),
        "Codex CLI exited before app-server initialized. This version may not support `codex app-server`; update Codex CLI, then refresh here."
    );
}

#[test]
fn a_closed_app_server_after_initializing_keeps_its_own_message_whatever_the_exit() {
    let root = crate::paths::scratch_dir("codex-app-server-closed-late");
    let status = CliStub::new("codex")
        .exit(2)
        .cli(&root)
        .command()
        .status()
        .unwrap();
    let error = QueryError {
        stage: QueryStage::Account,
        kind: QueryErrorKind::TransportClosed,
        message: "Codex app-server closed before returning limits.".to_string(),
    };

    assert_eq!(
        classify_query_failure(error, Some(status)),
        "Codex app-server closed before returning limits."
    );
}

#[test]
fn a_malformed_codex_answer_names_its_method() {
    let message = read_failure(scripted(vec![
        initialized(),
        chatgpt(),
        json!({"id": 3, "result": {"unexpected": true}}),
    ]));

    assert!(
        message.starts_with(
            "Codex app-server `account/rateLimits/read` returned a malformed response: "
        ),
        "{message}"
    );
}

#[test]
fn codex_protocol_errors_name_the_method_the_code_and_any_update_guidance() {
    for (reply, expected) in [
        (
            json!({"id": 3, "error": {"code": -32601, "message": "Method not found"}}),
            "Codex app-server `account/rateLimits/read` failed (code -32601): Method not found. Update Codex CLI to a version that supports subscription limits, then refresh here.",
        ),
        (
            json!({"id": 3, "error": {"code": -32000, "message": "busy"}}),
            "Codex app-server `account/rateLimits/read` failed (code -32000): busy.",
        ),
        (
            json!({"id": 3, "error": {}}),
            "Codex app-server `account/rateLimits/read` failed: unknown protocol error.",
        ),
    ] {
        assert_eq!(
            read_failure(scripted(vec![initialized(), chatgpt(), reply])),
            expected
        );
    }
    assert_eq!(
        read_failure(scripted(vec![
            initialized(),
            json!({"id": 2, "error": {"message": "boom"}}),
        ])),
        "Codex app-server `account/read` failed: boom."
    );
}

#[test]
fn a_cli_without_banked_resets_gets_the_whole_update_message() {
    let (outcome, _) = spend(
        "acct-1",
        |_| signed_in_as("acct-1"),
        consume_transport(
            chatgpt_account(),
            Some(json!({"id": 4, "error": {"code": -32601, "message": "Method not found"}})),
        ),
    );

    assert_eq!(
        outcome.unwrap_err(),
        "Codex app-server `account/rateLimitResetCredit/consume` failed (code -32601): Method not found. Update Codex CLI to a version that supports banked resets, then try again."
    );
}

#[test]
fn an_app_server_on_another_home_names_both_homes() {
    let message = read_failure(scripted(vec![
        json!({"id": 1, "result": {"codexHome": "/elsewhere/.codex"}}),
    ]));

    assert_eq!(
        message,
        "Codex app-server used /elsewhere/.codex, expected /fixture/.codex."
    );
}

#[test]
fn a_read_writes_these_exact_lines() {
    let mut transport = scripted(vec![
        initialized(),
        chatgpt(),
        json!({"id": 3, "result": {"rateLimits": {}}}),
    ]);

    query_app_server(Path::new("/fixture/.codex"), true, &mut transport).unwrap();

    let lines: Vec<String> = transport
        .sent
        .iter()
        .map(|message| serde_json::to_string(message).unwrap())
        .collect();
    assert_eq!(
        lines,
        [
            format!(
                r#"{{"id":1,"method":"initialize","params":{{"clientInfo":{{"name":"on_n_off","title":"on-n-off","version":"{}"}}}}}}"#,
                env!("CARGO_PKG_VERSION")
            ),
            r#"{"method":"initialized","params":{}}"#.to_string(),
            r#"{"id":2,"method":"account/read","params":{"refreshToken":true}}"#.to_string(),
            r#"{"id":3,"method":"account/rateLimits/read","params":{}}"#.to_string(),
        ]
    );
}
