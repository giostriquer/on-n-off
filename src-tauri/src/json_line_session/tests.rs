use super::*;
use crate::cli_stub::{CliStub, ANSWER_DEADLINE};

const LITE: Peer = Peer::jsonrpc_lite("Acme server", "Acme CLI exited early; update it.");

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

fn initialize_then(rest: impl IntoIterator<Item = Value>) -> Script {
    Script::new(std::iter::once(json!({"id": 1, "result": {}})).chain(rest))
}

fn opened<T>(
    session: Session,
    talk: impl FnOnce(&mut Conversation) -> Result<T, Failure>,
) -> Result<T, String> {
    session.run(|_: &Value| Ok(()), |_, conversation| talk(conversation))
}

#[test]
fn a_process_that_never_answers_times_out_and_finishing_stops_it() {
    let started = Instant::now();
    let mut process = process(
        &CliStub::new("acme").sleep(5),
        "json-line-timeout",
        LITE.within(Duration::from_millis(100), 1024),
    );

    let error = process.receive().unwrap_err();
    process.finish();

    assert_eq!(error.message, "Acme server timed out.");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(process.child.try_wait().unwrap().is_some());
}

#[test]
fn an_oversized_stdout_line_is_rejected_with_its_limit() {
    let mut process = process(
        &CliStub::new("acme").stdout(&"x".repeat(128)),
        "json-line-output-limit",
        LITE.within(ANSWER_DEADLINE, 64),
    );

    let error = process.receive().unwrap_err();
    process.finish();

    assert_eq!(error.message, "Acme server output line exceeded 64 bytes.");
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
fn a_peer_allows_a_mebibyte_line_and_thirty_seconds_by_default() {
    assert_eq!(LITE.line_limit, 1024 * 1024);
    assert_eq!(LITE.timeout, Duration::from_secs(30));
}

#[test]
fn a_cli_that_fills_its_stderr_pipe_still_reaches_its_answer() {
    let root = crate::paths::scratch_dir("json-line-stderr-flood");
    std::fs::write(
        root.join("noise.txt"),
        format!("{}\n", "e".repeat(255)).repeat(1024),
    )
    .unwrap();
    let stub = CliStub::new("acme")
        .stderr_file("noise.txt")
        .stdout(r#"{"id":7}"#);
    let mut process = match Process::spawn(
        LITE.within(Duration::from_secs(5), 1024),
        &mut stub.cli(&root).command(),
    ) {
        Ok(process) => process,
        Err(error) => panic!("{error:?}"),
    };

    let message = process.receive();
    process.finish();

    assert_eq!(message.unwrap(), json!({"id": 7}));
}

#[cfg(unix)]
#[test]
fn a_request_reaches_the_cli_as_one_line_and_closing_stdin_lets_it_exit_cleanly() {
    use std::os::unix::fs::PermissionsExt;
    let root = crate::paths::scratch_dir("json-line-stdin");
    let cli = root.join("acme");
    std::fs::write(
        &cli,
        concat!(
            "#!/bin/sh\n",
            "here=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\n",
            "head -n 1 > \"$here/received\"\n",
            "printf '%s\\n' '{\"id\":1,\"result\":{}}'\n",
            "cat > /dev/null\n",
            "exit 0\n",
        ),
    )
    .unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut process = match Process::spawn(
        LITE.within(Duration::from_secs(10), 1024),
        &mut Command::new(&cli),
    ) {
        Ok(process) => process,
        Err(error) => panic!("{error:?}"),
    };
    let request = json!({"id": 1, "method": "initialize", "params": {"clientInfo": {}}});

    process.send(&request).unwrap();
    let answer = process.receive();
    let exit = process.finish();

    assert_eq!(answer.unwrap(), json!({"id": 1, "result": {}}));
    assert!(exit.is_some_and(|status| status.success()), "{exit:?}");
    assert_eq!(
        std::fs::read_to_string(root.join("received")).unwrap(),
        "{\"id\":1,\"method\":\"initialize\",\"params\":{\"clientInfo\":{}}}\n"
    );
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
fn dropping_a_process_kills_its_child() {
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

    let error = opened(session, |_| Ok(())).unwrap_err();

    assert_eq!(error, "Acme CLI exited early; update it.");
}

#[test]
fn a_cli_that_answers_initialize_and_then_exits_unsuccessfully_keeps_the_closed_message() {
    let stub = CliStub::new("acme")
        .awaits_line()
        .stdout(r#"{"id":1,"result":{}}"#)
        .sleep(3)
        .exit(2);
    let Ok(session) = Session::start(LITE, command(&stub, "json-line-late-exit")) else {
        panic!("the stub did not start");
    };

    let error = opened(session, |conversation| {
        conversation.request::<Value>(Request::new(2, "acme/read"))
    })
    .unwrap_err();

    assert_eq!(error, "Acme server closed before returning limits.");
}

#[test]
fn a_request_to_a_cli_that_has_already_exited_is_closed_and_gets_the_guidance() {
    let mut process = process(
        &CliStub::new("acme").exit(2),
        "json-line-write-exited",
        LITE,
    );
    let started = Instant::now();
    let status = loop {
        if let Some(status) = process.child.try_wait().unwrap() {
            break status;
        }
        assert!(started.elapsed() < ANSWER_DEADLINE, "the stub never exited");
        thread::sleep(Duration::from_millis(10));
    };

    let error = process
        .send(&json!({"id": 1, "method": "initialize", "params": {}}))
        .unwrap_err();

    assert!(
        error
            .message
            .starts_with("Could not write to Acme server: "),
        "{}",
        error.message
    );
    assert_eq!(
        explained(LITE, error, Some(status)),
        "Acme CLI exited early; update it."
    );
}

#[test]
fn an_early_broken_pipe_and_an_unsuccessful_exit_give_the_same_guidance() {
    let message = explained(
        LITE,
        Failure::closed("Could not write to Acme server: Broken pipe"),
        Some(exit_status(2)),
    );

    assert_eq!(message, "Acme CLI exited early; update it.");
}

#[test]
fn an_unsuccessful_exit_does_not_hide_a_failure_other_than_a_closed_pipe() {
    assert_eq!(
        explained(
            LITE,
            Failure::new("Acme server timed out."),
            Some(exit_status(2))
        ),
        "Acme server timed out."
    );
}

#[test]
fn a_peer_that_closes_before_initializing_and_exits_cleanly_keeps_its_own_message() {
    let message = "Acme server closed before returning limits.";

    assert_eq!(
        explained(LITE, Failure::closed(message), Some(exit_status(0))),
        message
    );
    assert_eq!(explained(LITE, Failure::closed(message), None), message);
}

#[test]
fn a_request_waits_for_the_answer_with_its_id() {
    let script = initialize_then([
        json!({"method": "status/changed", "params": {}}),
        json!({"id": 9, "result": "someone else's"}),
        json!({"id": 2, "result": {"ok": true}}),
        json!({"id": 3, "result": "never read"}),
    ]);

    let answer: Value = opened(script.session(LITE), |conversation| {
        conversation.request(Request::new(2, "acme/read"))
    })
    .unwrap();

    assert_eq!(answer, json!({"ok": true}));
    assert_eq!(
        script.log()[3..],
        [
            Exchange::Sent(json!({"id": 2, "method": "acme/read", "params": {}})),
            Exchange::Received(json!({"method": "status/changed", "params": {}})),
            Exchange::Received(json!({"id": 9, "result": "someone else's"})),
            Exchange::Received(json!({"id": 2, "result": {"ok": true}})),
        ]
    );
}

#[test]
fn the_handshake_sends_only_the_client_and_every_line_is_bare_json() {
    let script = initialize_then([json!({"id": 2, "result": {}})]);

    opened(script.session(LITE), |conversation| {
        conversation.request::<Value>(Request::new(2, "acme/read").params(json!({"a": 1})))
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
                r#"{{"id":1,"method":"initialize","params":{{"clientInfo":{{"name":"on_n_off","title":"on-n-off","version":"{}"}}}}}}"#,
                env!("CARGO_PKG_VERSION")
            ),
            r#"{"method":"initialized","params":{}}"#.to_string(),
            r#"{"id":2,"method":"acme/read","params":{"a":1}}"#.to_string(),
        ]
    );
}

#[test]
fn a_rejected_answer_ends_the_handshake_before_initialized_with_its_own_message() {
    let script = Script::new([json!({"id": 1, "result": {"home": "/elsewhere"}})]);
    let talked = std::cell::Cell::new(false);

    let error = script
        .session(LITE)
        .run(
            |answer: &Value| {
                Err(format!(
                    "Acme server used {}.",
                    answer["home"].as_str().unwrap()
                ))
            },
            |_, _| {
                talked.set(true);
                Ok(())
            },
        )
        .unwrap_err();

    assert!(!talked.get());

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

    let error = opened(script.session(LITE), |_| {
        Err::<(), _>(Failure::new("The signed-in account changed."))
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

        let error = opened(initialize_then([reply]).session(LITE), |conversation| {
            conversation.request::<Value>(request)
        })
        .unwrap_err();

        assert_eq!(error, expected);
    }
}

#[test]
fn a_malformed_answer_names_the_peer_and_the_method() {
    let error = opened(
        initialize_then([json!({"id": 2, "result": "many"})]).session(LITE),
        |conversation| conversation.request::<u32>(Request::new(2, "acme/count")),
    )
    .unwrap_err();

    assert!(
        error.starts_with("Acme server `acme/count` returned a malformed response: "),
        "{error}"
    );
}
