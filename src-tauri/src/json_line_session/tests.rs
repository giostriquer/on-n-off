use super::*;
use crate::cli_stub::{CliStub, ANSWER_DEADLINE};

const LITE: Peer = Peer::jsonrpc_lite("Acme server", "Acme CLI exited early; update it.");
const STRICT: Peer = Peer::jsonrpc("Webapp serve", "Webapp CLI exited early; update it.");

fn command(stub: &CliStub, name: &str) -> Command {
    stub.cli(&crate::paths::scratch_dir(name)).command()
}

fn process(stub: &CliStub, name: &str, peer: Peer) -> Process {
    match Process::spawn(peer, &mut command(stub, name)) {
        Ok(process) => process,
        Err(error) => panic!("{error:?}"),
    }
}

fn exit_status(code: i32) -> ExitStatus {
    command(&CliStub::new("acme").exit(code), "json-line-exit-status")
        .status()
        .unwrap()
}

fn closed(message: &str) -> Failure {
    Failure::new(FailureKind::Closed, message)
}

fn initialize_then(rest: impl IntoIterator<Item = Value>) -> Script {
    Script::new(std::iter::once(json!({"id": 1, "result": {}})).chain(rest))
}

#[test]
fn a_session_that_never_answers_times_out_and_stops_the_child() {
    let started = Instant::now();
    let mut process = process(
        &CliStub::new("acme").sleep(5),
        "json-line-timeout",
        LITE.within(Duration::from_millis(100), 1024),
    );

    let error = process.receive().unwrap_err();
    process.finish();

    assert_eq!(error.kind, FailureKind::Timeout);
    assert_eq!(error.message, "Acme server timed out.");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(process.child.try_wait().unwrap().is_some());
}

#[test]
fn an_oversized_stdout_line_is_rejected_with_its_limit() {
    let mut process = process(
        &CliStub::new("acme").stdout(&"x".repeat(128)),
        "json-line-output-limit",
        STRICT.within(ANSWER_DEADLINE, 64),
    );

    let error = process.receive().unwrap_err();
    process.finish();

    assert_eq!(error.kind, FailureKind::InvalidOutput);
    assert_eq!(error.message, "Webapp serve output line exceeded 64 bytes.");
}

#[test]
fn a_line_within_the_limit_is_read() {
    let mut process = process(
        &CliStub::new("acme").stdout(r#"{"id":7}"#),
        "json-line-output-within-limit",
        LITE.within(ANSWER_DEADLINE, 64),
    );

    let message = process.receive();
    process.finish();

    assert_eq!(message.unwrap(), json!({"id": 7}));
}

#[test]
fn stderr_never_reaches_a_message() {
    let mut process = process(
        &CliStub::new("acme")
            .stdout("not-json")
            .stderr("sensitive-provider-diagnostic"),
        "json-line-stderr",
        LITE,
    );

    let error = process.receive().unwrap_err();
    process.finish();

    assert_eq!(error.kind, FailureKind::InvalidOutput);
    assert!(
        error
            .message
            .starts_with("Acme server returned invalid JSON: "),
        "{}",
        error.message
    );
    assert!(!error.message.contains("sensitive-provider-diagnostic"));
}

#[test]
fn a_missing_cli_is_missing_and_not_a_failure() {
    let root = crate::paths::scratch_dir("json-line-missing");

    let started = Session::start(LITE, Command::new(root.join("acme")));

    assert!(matches!(started, Err(NotStarted::Missing)));
}

#[cfg(unix)]
#[test]
fn a_cli_that_cannot_start_is_named() {
    let root = crate::paths::scratch_dir("json-line-not-executable");
    let path = root.join("acme");
    std::fs::write(&path, "#!/bin/sh\n").unwrap();

    let started = Session::start(LITE, Command::new(&path));

    match started {
        Err(NotStarted::Failed(message)) => {
            assert!(
                message.starts_with("Could not start Acme server: "),
                "{message}"
            );
        }
        Err(NotStarted::Missing) => panic!("a file that is there is not missing"),
        Ok(_) => panic!("a file without the executable bit started"),
    }
}

#[cfg(unix)]
#[test]
fn dropping_a_session_stops_its_child() {
    let process = process(&CliStub::new("acme").sleep(30), "json-line-drop", LITE);
    let pid = process.child.id().to_string();

    drop(process);

    let alive = Command::new("kill")
        .args(["-0", &pid])
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!alive.success(), "process {pid} outlived its session");
}

#[test]
fn an_unsuccessful_exit_before_initialize_is_answered_gets_the_peers_guidance() {
    let stub = CliStub::new("acme").exit(2);
    let Ok(session) = Session::start(LITE, command(&stub, "json-line-early-exit")) else {
        panic!("the stub did not start");
    };

    let error = session
        .run(|session| session.handshake::<Value>(None, |_| Ok(())))
        .unwrap_err();

    assert_eq!(error, "Acme CLI exited early; update it.");
}

#[test]
fn an_early_broken_pipe_and_an_unsuccessful_exit_give_the_same_guidance() {
    let message = explained(
        STRICT,
        false,
        closed("Could not write to Webapp serve: Broken pipe"),
        Some(exit_status(2)),
    );

    assert_eq!(message, "Webapp CLI exited early; update it.");
}

#[test]
fn an_unsuccessful_exit_does_not_hide_a_timeout_or_invalid_output() {
    for (kind, message) in [
        (FailureKind::Timeout, "Acme server timed out."),
        (
            FailureKind::InvalidOutput,
            "Acme server returned invalid JSON.",
        ),
        (
            FailureKind::Protocol,
            "Acme server `initialize` failed: no.",
        ),
        (FailureKind::Rejected, "Acme server used the wrong home."),
    ] {
        assert_eq!(
            explained(
                LITE,
                false,
                Failure::new(kind, message),
                Some(exit_status(2))
            ),
            message
        );
    }
}

#[test]
fn a_peer_that_closes_after_initializing_or_exits_cleanly_keeps_its_own_message() {
    let message = "Acme server closed before returning limits.";

    assert_eq!(
        explained(LITE, true, closed(message), Some(exit_status(2))),
        message
    );
    assert_eq!(
        explained(LITE, false, closed(message), Some(exit_status(0))),
        message
    );
    assert_eq!(explained(LITE, false, closed(message), None), message);
}

#[test]
fn a_request_waits_for_the_answer_with_its_id() {
    let script = Script::new([
        json!({"method": "status/changed", "params": {}}),
        json!({"id": 9, "result": "someone else's"}),
        json!({"id": 2, "result": {"ok": true}}),
        json!({"id": 3, "result": "never read"}),
    ]);

    let answer: Value = script
        .session(LITE)
        .run(|session| session.request(Request::new(2, "acme/read")))
        .unwrap();

    assert_eq!(answer, json!({"ok": true}));
    assert_eq!(
        script.log(),
        [
            Exchange::Sent(json!({"id": 2, "method": "acme/read", "params": {}})),
            Exchange::Received(json!({"method": "status/changed", "params": {}})),
            Exchange::Received(json!({"id": 9, "result": "someone else's"})),
            Exchange::Received(json!({"id": 2, "result": {"ok": true}})),
        ]
    );
}

#[test]
fn a_lite_peer_gets_bare_lines_and_a_strict_peer_gets_jsonrpc_on_every_line() {
    for (peer, stamp) in [(LITE, ""), (STRICT, r#""jsonrpc":"2.0","#)] {
        let script = initialize_then([json!({"id": 2, "result": {}})]);

        script
            .session(peer)
            .run(|session| {
                session.handshake::<Value>(Some(json!({"acme": true})), |_| Ok(()))?;
                session.request::<Value>(Request::new(2, "acme/read").params(json!({"a": 1})))
            })
            .unwrap();

        let lines: Vec<String> = script
            .sent()
            .iter()
            .map(|message| serde_json::to_string(message).unwrap())
            .collect();
        assert_eq!(
            lines,
            [
                format!(
                    r#"{{{stamp}"id":1,"method":"initialize","params":{{"clientInfo":{{"name":"on_n_off","title":"on-n-off","version":"{}"}},"capabilities":{{"acme":true}}}}}}"#,
                    env!("CARGO_PKG_VERSION")
                ),
                format!(r#"{{{stamp}"method":"initialized","params":{{}}}}"#),
                format!(r#"{{{stamp}"id":2,"method":"acme/read","params":{{"a":1}}}}"#),
            ]
        );
    }
}

#[test]
fn a_handshake_without_capabilities_sends_only_the_client() {
    let script = initialize_then([]);

    script
        .session(LITE)
        .run(|session| session.handshake::<Value>(None, |_| Ok(())))
        .unwrap();

    assert_eq!(
        script.sent()[0]["params"],
        json!({"clientInfo": {
            "name": "on_n_off",
            "title": "on-n-off",
            "version": env!("CARGO_PKG_VERSION"),
        }})
    );
}

#[test]
fn a_rejected_answer_ends_the_handshake_before_initialized_with_its_own_message() {
    let script = Script::new([json!({"id": 1, "result": {"home": "/elsewhere"}})]);

    let error = script
        .session(LITE)
        .run(|session| {
            session.handshake::<Value>(None, |answer| {
                Err(format!(
                    "Acme server used {}.",
                    answer["home"].as_str().unwrap()
                ))
            })
        })
        .unwrap_err();

    assert_eq!(error, "Acme server used /elsewhere.");
    let methods: Vec<Value> = script
        .sent()
        .iter()
        .map(|message| message["method"].clone())
        .collect();
    assert_eq!(methods, [json!("initialize")]);
}

#[test]
fn a_reader_can_reject_mid_session_with_its_own_message() {
    let script = initialize_then([]);

    let error = script
        .session(LITE)
        .run(|session| {
            session.handshake::<Value>(None, |_| Ok(()))?;
            Err::<(), _>(Failure::rejected("The signed-in account changed."))
        })
        .unwrap_err();

    assert_eq!(error, "The signed-in account changed.");
}

#[test]
fn a_protocol_error_names_the_peer_the_method_and_the_code() {
    for (reply, when_missing, expected) in [
        (
            json!({"id": 2, "error": {"code": -32601, "message": "Method not found"}}),
            Some("Update Acme."),
            "Acme server `acme/read` failed (code -32601): Method not found. Update Acme.",
        ),
        (
            json!({"id": 2, "error": {"code": -32601, "message": "Method not found"}}),
            None,
            "Acme server `acme/read` failed (code -32601): Method not found.",
        ),
        (
            json!({"id": 2, "error": {"code": -32000, "message": "busy"}}),
            Some("Update Acme."),
            "Acme server `acme/read` failed (code -32000): busy.",
        ),
        (
            json!({"id": 2, "error": {"message": "boom"}}),
            Some("Update Acme."),
            "Acme server `acme/read` failed: boom.",
        ),
        (
            json!({"id": 2}),
            None,
            "Acme server `acme/read` failed: unknown protocol error.",
        ),
    ] {
        let request = Request::new(2, "acme/read");
        let request = match when_missing {
            Some(guidance) => request.when_missing(guidance),
            None => request,
        };

        let error = Script::new([reply])
            .session(LITE)
            .run(|session| session.request::<Value>(request))
            .unwrap_err();

        assert_eq!(error, expected);
    }
}

#[test]
fn a_malformed_answer_names_the_peer_and_the_method() {
    let error = Script::new([json!({"id": 2, "result": "many"})])
        .session(STRICT)
        .run(|session| session.request::<u32>(Request::new(2, "webapp/count")))
        .unwrap_err();

    assert!(
        error.starts_with("Webapp serve `webapp/count` returned a malformed response: "),
        "{error}"
    );
}
