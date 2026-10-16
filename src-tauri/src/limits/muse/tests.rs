use super::*;
use crate::dto::{LimitWindowKind, LimitsStatus};
use crate::json_line_session::Script;
use serde_json::{json, Value};
use std::path::PathBuf;

fn query(script: &Script) -> Result<ServeAnswers, String> {
    query_serve(script.session(SERVE), &data_home())
}

fn data_home() -> PathBuf {
    PathBuf::from("/fixture/.local/share/muse")
}

fn initialized() -> Value {
    json!({"id": 1, "result": {
        "museHome": "/fixture/.local/share/muse",
        "experimentalApi": true,
        "platformOs": "macos"
    }})
}

fn account(state: &str, label: Option<&str>) -> Value {
    let mut result = json!({"state": state, "credentialRequired": true});
    if let Some(label) = label {
        result["label"] = json!(label);
    }
    result
}

fn happy_replies() -> Vec<Value> {
    vec![
        json!({"method": "session/listChanged", "params": {}}),
        initialized(),
        json!({"id": 2, "result": account("accountLogin", Some("You@Example.com"))}),
        json!({"id": 3, "result": {"usage": {
            "tier": "wonder",
            "window": {"usedPercent": 42, "windowDurationMins": 300, "resetsAtMs": 1_760_000_000_000i64},
            "weekly": {"usedPercent": 7, "resetsAtMs": 1_760_500_000_000i64},
            "observedAtMs": 1_759_999_000_000i64
        }}}),
        json!({"id": 4, "result": account("accountLogin", Some("You@Example.com"))}),
    ]
}

#[test]
fn handshake_requests_the_experimental_api_and_reads_usage() {
    let script = Script::new(happy_replies());
    let answers = query(&script).expect("query");
    let parsed = normalize(answers).expect("normalize");

    assert_eq!(
        script.sent()[0],
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "clientInfo": {"name": "on_n_off", "title": "on-n-off", "version": env!("CARGO_PKG_VERSION")},
            "capabilities": {"experimentalApi": true, "userInputDialogs": false}
        }})
    );
    let sent = script.sent();
    let methods: Vec<_> = sent
        .iter()
        .filter_map(|message| message.get("method").and_then(Value::as_str))
        .collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "initialized",
            "account/read",
            "usage/read",
            "account/read"
        ]
    );

    let account = parsed.account.expect("account");
    assert_eq!(account.id, "email:you@example.com");
    assert_eq!(account.label.as_deref(), Some("You@Example.com"));
    assert_eq!(parsed.reading.plan.as_deref(), Some("wonder"));
    assert_eq!(parsed.reading.windows.len(), 2);
    let session_window = parsed
        .reading
        .windows
        .iter()
        .find(|window| window.kind == LimitWindowKind::Session)
        .expect("session window");
    assert_eq!(session_window.id, "session");
    assert_eq!(session_window.label, "5 hour · all models");
    assert_eq!(session_window.used_percent, 42.0);
    assert_eq!(session_window.window_seconds, Some(18_000));
    assert_eq!(
        session_window.resets_at.as_deref(),
        Some("2025-10-09T08:53:20.000Z")
    );
    assert_eq!(session_window.observed_at, "2025-10-09T08:36:40.000Z");
    let weekly = parsed
        .reading
        .windows
        .iter()
        .find(|window| window.kind == LimitWindowKind::Weekly)
        .expect("weekly window");
    assert_eq!(weekly.id, "weekly");
    assert_eq!(weekly.label, "Weekly · all models");
    assert_eq!(weekly.used_percent, 7.0);
}

#[test]
fn an_opaque_numeric_tier_is_not_a_plan_name() {
    let mut replies = happy_replies();
    replies[3]["result"]["usage"]["tier"] = json!("27681631238169137");
    let answers = query(&Script::new(replies)).expect("query");
    let parsed = normalize(answers).expect("normalize");
    assert_eq!(parsed.reading.plan, None);
    assert_eq!(parsed.reading.windows.len(), 2, "windows still read");
}

#[test]
fn a_different_muse_home_fails_the_read() {
    let script = Script::new([json!({"id": 1, "result": {
        "museHome": "/somewhere/else/muse",
        "experimentalApi": true
    }})]);
    let error = query(&script).expect_err("home mismatch");
    assert!(error.contains("expected"), "{error:?}");
}

#[test]
fn a_host_without_the_experimental_api_fails_with_update_guidance() {
    let script = Script::new([json!({"id": 1, "result": {
        "museHome": "/fixture/.local/share/muse",
        "experimentalApi": false
    }})]);
    let error = query(&script).expect_err("no experimental api");
    assert!(error.contains("Update Muse"), "{error:?}");
}

#[test]
fn signed_out_and_api_key_states_map_to_their_cards() {
    for (state, status, needle) in [
        ("loggedOut", LimitsStatus::SignedOut, "Sign in with `muse`"),
        ("apiKey", LimitsStatus::Unsupported, "API key"),
        ("envKey", LimitsStatus::Unsupported, "API key"),
    ] {
        let script = Script::new([
            initialized(),
            json!({"id": 2, "result": account(state, None)}),
        ]);
        let answers = query(&script).expect("query");
        let card = finish_read(normalize(answers));
        assert_eq!(card.status, status, "{state}");
        assert!(
            card.message.as_deref().is_some_and(|m| m.contains(needle)),
            "{state}: {:?}",
            card.message
        );
        assert!(card.reading.windows.is_empty());
    }
}

#[test]
fn unobserved_usage_keeps_the_account_on_a_failed_card() {
    let script = Script::new([
        initialized(),
        json!({"id": 2, "result": account("accountLogin", Some("you@example.com"))}),
        json!({"id": 3, "result": {}}),
        json!({"id": 4, "result": account("accountLogin", Some("you@example.com"))}),
    ]);
    let answers = query(&script).expect("query");
    let card = finish_read(normalize(answers));
    assert_eq!(card.status, LimitsStatus::Failed);
    assert_eq!(
        card.account.as_ref().map(|account| account.id.as_str()),
        Some("email:you@example.com")
    );
    assert!(
        card.message
            .as_deref()
            .is_some_and(|m| m.contains("has not observed")),
        "{:?}",
        card.message
    );
}

#[test]
fn an_account_change_during_the_read_fails() {
    let mut replies = happy_replies();
    let last = replies.len() - 1;
    replies[last] = json!({"id": 4, "result": account("accountLogin", Some("other@example.com"))});
    let error = query(&Script::new(replies)).expect_err("changed account");
    assert!(error.contains("changed"), "{error:?}");
}

#[test]
fn a_missing_usage_method_asks_for_an_update() {
    let script = Script::new([
        initialized(),
        json!({"id": 2, "result": account("accountLogin", Some("you@example.com"))}),
        json!({"id": 3, "error": {"code": -32601, "message": "method not found"}}),
    ]);
    let error = query(&script).expect_err("method missing");
    assert!(error.contains("Update Muse"), "{error:?}");
    assert!(error.contains("usage/read"), "{error:?}");
}

fn serve(
    stub: crate::cli_stub::CliStub,
    name: &str,
    timeout: std::time::Duration,
    line_limit: usize,
) -> Session {
    let root = crate::paths::scratch_dir(name);
    match begin(SERVE.within(timeout, line_limit), stub.cli(&root).command()) {
        Ok(session) => session,
        Err(error) => panic!("{error:?}"),
    }
}

fn read_failure(session: Session) -> String {
    query_serve(session, &data_home()).unwrap_err()
}

fn scripted(replies: Vec<Value>) -> Session {
    Script::new(replies).session(SERVE)
}

#[test]
fn a_missing_muse_cli_reads_as_not_installed() {
    let root = crate::paths::scratch_dir("muse-serve-missing");
    let missing = root.join("muse");

    let error = begin(SERVE, Command::new(&missing)).err();
    let card = finish_read(read_with(&root, |_, _, _| {
        begin(SERVE, Command::new(&missing))
    }));

    assert_eq!(error, Some(ServeFailure::NotInstalled));
    assert_eq!(card.status, LimitsStatus::SignedOut);
    assert_eq!(
        card.message.as_deref(),
        Some("Install Muse Code and sign in with `muse` to see subscription limits.")
    );
}

#[test]
fn a_muse_serve_that_never_answers_times_out_by_name() {
    let session = serve(
        crate::cli_stub::CliStub::new("muse").sleep(5),
        "muse-serve-timeout-message",
        std::time::Duration::from_millis(100),
        1024,
    );

    assert_eq!(read_failure(session), "Muse serve timed out.");
}

#[test]
fn an_oversized_muse_serve_line_is_named_with_its_limit() {
    let session = serve(
        crate::cli_stub::CliStub::new("muse")
            .awaits_line()
            .stdout(&"x".repeat(128)),
        "muse-serve-line-message",
        crate::cli_stub::ANSWER_DEADLINE,
        64,
    );

    assert_eq!(
        read_failure(session),
        "Muse serve output line exceeded 64 bytes."
    );
}

#[test]
fn muse_serve_output_that_is_not_json_is_named() {
    let session = serve(
        crate::cli_stub::CliStub::new("muse")
            .awaits_line()
            .stdout("not-json"),
        "muse-serve-json-message",
        crate::cli_stub::ANSWER_DEADLINE,
        1024,
    );

    let message = read_failure(session);

    assert!(
        message.starts_with("Muse serve returned invalid JSON: "),
        "{message}"
    );
}

#[test]
fn a_muse_serve_that_closes_cleanly_before_answering_says_so() {
    let session = serve(
        crate::cli_stub::CliStub::new("muse").awaits_line(),
        "muse-serve-closed-message",
        crate::cli_stub::ANSWER_DEADLINE,
        1024,
    );

    assert_eq!(
        read_failure(session),
        "Muse serve closed before returning limits."
    );
}

#[test]
fn a_muse_cli_that_exits_unsuccessfully_before_initializing_is_asked_to_update() {
    let session = serve(
        crate::cli_stub::CliStub::new("muse").exit(2),
        "muse-serve-exit-message",
        crate::cli_stub::ANSWER_DEADLINE,
        1024,
    );

    assert_eq!(
        read_failure(session),
        "Muse CLI exited before `muse serve` initialized. This version may not support `muse serve`; update Muse Code, then refresh here."
    );
}

#[test]
fn a_malformed_muse_answer_names_its_method() {
    let message = read_failure(scripted(vec![
        initialized(),
        json!({"id": 2, "result": account("accountLogin", Some("you@example.com"))}),
        json!({"id": 3, "result": {"usage": {"tier": 7}}}),
    ]));

    assert!(
        message.starts_with("Muse serve `usage/read` returned a malformed response: "),
        "{message}"
    );
}

#[test]
fn muse_protocol_errors_name_the_method_the_code_and_any_update_guidance() {
    let signed_in = json!({"id": 2, "result": account("accountLogin", Some("you@example.com"))});
    for (reply, expected) in [
        (
            json!({"id": 3, "error": {"code": -32601, "message": "method not found"}}),
            "Muse serve `usage/read` failed (code -32601): method not found. Update Muse to a version that reports subscription usage, then refresh here.",
        ),
        (
            json!({"id": 3, "error": {"code": -32000, "message": "busy"}}),
            "Muse serve `usage/read` failed (code -32000): busy.",
        ),
        (
            json!({"id": 3, "error": {}}),
            "Muse serve `usage/read` failed: unknown protocol error.",
        ),
    ] {
        assert_eq!(
            read_failure(scripted(vec![initialized(), signed_in.clone(), reply])),
            expected
        );
    }
    assert_eq!(
        read_failure(scripted(vec![
            initialized(),
            json!({"id": 2, "error": {"code": -32601, "message": "method not found"}}),
        ])),
        "Muse serve `account/read` failed (code -32601): method not found."
    );
}

#[test]
fn muse_handshake_and_account_refusals_read_exactly() {
    assert_eq!(
        read_failure(scripted(vec![json!({"id": 1, "result": {
            "museHome": "/somewhere/else/muse",
            "experimentalApi": true
        }})])),
        "Muse serve used /somewhere/else/muse, expected /fixture/.local/share/muse."
    );
    assert_eq!(
        read_failure(scripted(vec![json!({"id": 1, "result": {
            "museHome": "/fixture/.local/share/muse"
        }})])),
        "Update Muse: this version cannot report subscription usage to on-n-off."
    );
    let mut changed = happy_replies();
    let last = changed.len() - 1;
    changed[last] = json!({"id": 4, "result": account("accountLogin", Some("other@example.com"))});
    assert_eq!(
        read_failure(scripted(changed)),
        "The signed-in Muse account changed while its usage was read."
    );
}

#[test]
fn a_muse_read_writes_these_exact_lines() {
    let script = Script::new(happy_replies());

    query(&script).unwrap();

    let lines: Vec<String> = script
        .sent()
        .iter()
        .map(|message| serde_json::to_string(message).unwrap())
        .collect();
    assert_eq!(
        lines,
        [
            format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"clientInfo":{{"name":"on_n_off","title":"on-n-off","version":"{}"}},"capabilities":{{"experimentalApi":true,"userInputDialogs":false}}}}}}"#,
                env!("CARGO_PKG_VERSION")
            ),
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
            r#"{"jsonrpc":"2.0","id":2,"method":"account/read","params":{}}"#.to_string(),
            r#"{"jsonrpc":"2.0","id":3,"method":"usage/read","params":{}}"#.to_string(),
            r#"{"jsonrpc":"2.0","id":4,"method":"account/read","params":{}}"#.to_string(),
        ]
    );
}

fn config_with_auth(body: &str) -> PathBuf {
    let config = crate::paths::scratch_dir("muse-mint-auth");
    std::fs::create_dir_all(&config).expect("config dir");
    std::fs::write(config.join("auth.json"), body).expect("auth.json");
    config
}

fn no_keychain() -> Option<String> {
    None
}

#[test]
fn an_inline_device_token_is_loaded_with_its_email() {
    let config = config_with_auth(
        r#"{"schema_version":2,"providers":{"meta":{"mechanism":"oauth","access_token":"device-token","user_email":"You@Example.com"}}}"#,
    );
    let login = mint::load_login(&config, &no_keychain).expect("login");
    assert_eq!(login.token, "device-token");
    assert_eq!(login.email.as_deref(), Some("You@Example.com"));
}

#[test]
fn a_keychain_login_uses_the_keychain_when_no_token_is_inline() {
    let config = config_with_auth(
        r#"{"schema_version":2,"providers":{"meta":{"mechanism":"oauth","storage":"keychain","user_email":"you@example.com"}}}"#,
    );
    let login = mint::load_login(&config, &|| Some("keychain-token".to_string())).expect("login");
    assert_eq!(login.token, "keychain-token");
    assert!(mint::load_login(&config, &no_keychain).is_none());
}

#[test]
fn a_keychain_bundle_yields_only_its_access_token() {
    let config = config_with_auth(
        r#"{"schema_version":2,"providers":{"meta":{"mechanism":"oauth","storage":"keychain","user_email":"you@example.com"}}}"#,
    );
    let bundle = r#"{
        "secret_schema_version": 1,
        "access_token": "bundle-access-token",
        "refresh_token": "bundle-refresh-secret",
        "api_key": "LLM|bundle-minted-secret"
    }"#;
    let login = mint::load_login(&config, &|| Some(bundle.to_string())).expect("login");
    assert_eq!(login.token, "bundle-access-token");
    let formatted = format!("{login:?}");
    assert!(!formatted.contains("bundle-access-token"), "{formatted}");
    assert!(!formatted.contains("bundle-refresh-secret"), "{formatted}");
    assert!(!formatted.contains("bundle-minted-secret"), "{formatted}");

    let tokenless = r#"{"secret_schema_version":1,"refresh_token":"bundle-refresh-secret"}"#;
    assert!(mint::load_login(&config, &|| Some(tokenless.to_string())).is_none());
}

#[test]
fn the_cached_login_is_reread_only_when_auth_json_changes() {
    mint::reset_cache();
    let body = r#"{"schema_version":2,"providers":{"meta":{"mechanism":"oauth","storage":"keychain","user_email":"you@example.com"}}}"#;
    let config = config_with_auth(body);
    let calls = std::cell::Cell::new(0);
    let keychain = || {
        calls.set(calls.get() + 1);
        Some("keychain-token".to_string())
    };
    assert!(mint::load_login_cached(&config, &keychain).is_some());
    assert!(mint::load_login_cached(&config, &keychain).is_some());
    assert_eq!(calls.get(), 1, "one Keychain lookup while auth.json stands");

    let before = std::fs::metadata(config.join("auth.json"))
        .and_then(|metadata| metadata.modified())
        .ok();
    for _ in 0..100 {
        std::fs::write(config.join("auth.json"), body).expect("rewrite auth.json");
        let after = std::fs::metadata(config.join("auth.json"))
            .and_then(|metadata| metadata.modified())
            .ok();
        if after != before {
            break;
        }
    }
    assert!(mint::load_login_cached(&config, &keychain).is_some());
    assert_eq!(calls.get(), 2, "a rewritten auth.json re-reads the login");
    mint::reset_cache();
}

#[test]
fn no_login_means_no_mint() {
    let missing = crate::paths::scratch_dir("muse-mint-missing");
    assert!(mint::load_login(&missing, &|| Some("unused".to_string())).is_none());
    let api_key = config_with_auth(r#"{"providers":{"meta":{"mechanism":"apiKey"}}}"#);
    assert!(mint::load_login(&api_key, &|| Some("unused".to_string())).is_none());
    let corrupt = config_with_auth("{not json");
    assert!(mint::load_login(&corrupt, &no_keychain).is_none());
}

fn mint_body() -> Value {
    json!({
        "is_subs_active": true,
        "require_payment": false,
        "subs_tier_name": "Wonder",
        "subs_usage": {
            "tier": "27681631238169137",
            "window": {"used_percent": 41, "window_duration_mins": 300, "resets_at": 1_760_000_000i64},
            "weekly": {"used_percent": 22, "resets_at": 1_760_500_000i64}
        }
    })
}

#[test]
fn a_mint_response_normalizes_to_the_same_card_as_serve() {
    let login = mint::DeviceLogin {
        token: "device-token".to_string(),
        email: Some("You@Example.com".to_string()),
    };
    let response: mint::MintResponse = serde_json::from_value(mint_body()).expect("parse");
    let parsed = mint::normalize(&login, &response)
        .expect("mint reading")
        .expect("parsed");
    let account = parsed.account.expect("account");
    assert_eq!(account.id, "email:you@example.com");
    assert_eq!(account.label.as_deref(), Some("You@Example.com"));
    assert_eq!(parsed.reading.plan.as_deref(), Some("Wonder"));
    assert_eq!(parsed.reading.windows.len(), 2);
    let weekly = parsed
        .reading
        .windows
        .iter()
        .find(|window| window.kind == LimitWindowKind::Weekly)
        .expect("weekly");
    assert_eq!(weekly.used_percent, 22.0);
    assert_eq!(
        weekly.resets_at.as_deref(),
        Some("2025-10-15T03:46:40.000Z")
    );
    let session = parsed
        .reading
        .windows
        .iter()
        .find(|window| window.kind == LimitWindowKind::Session)
        .expect("session");
    assert_eq!(session.label, "5 hour · all models");
    assert_eq!(session.used_percent, 41.0);
    assert_eq!(session.window_seconds, Some(18_000));
    assert_eq!(
        session.resets_at.as_deref(),
        Some("2025-10-09T08:53:20.000Z")
    );
}

#[test]
fn an_idle_or_inactive_mint_response_maps_without_fake_windows() {
    let login = mint::DeviceLogin {
        token: "device-token".to_string(),
        email: None,
    };
    let idle: mint::MintResponse = serde_json::from_value(json!({
        "is_subs_active": true, "subs_tier_name": "Wonder", "subs_usage": null
    }))
    .expect("parse idle");
    assert!(mint::normalize(&login, &idle).is_none(), "idle falls back");

    let inactive: mint::MintResponse = serde_json::from_value(json!({
        "is_subs_active": false, "subs_tier_name": "Wonder"
    }))
    .expect("parse inactive");
    assert!(matches!(
        mint::normalize(&login, &inactive),
        Some(Err(ServeFailure::Unsupported(_)))
    ));

    let billing: mint::MintResponse = serde_json::from_value(json!({
        "is_subs_active": true, "require_payment": true
    }))
    .expect("parse billing");
    assert!(matches!(
        mint::normalize(&login, &billing),
        Some(Err(ServeFailure::Unsupported(_)))
    ));
}

#[test]
fn a_minted_key_in_the_response_is_never_kept() {
    let mut body = mint_body();
    body["api_key"] = json!("LLM|secret-minted-key");
    body["account"] = json!({"id": "acct-1"});
    let response: mint::MintResponse = serde_json::from_value(body).expect("parse");
    let debug = format!("{response:?}");
    assert!(!debug.contains("secret-minted-key"), "{debug}");
    assert!(!debug.contains("acct-1"), "{debug}");
}

#[test]
fn the_mint_call_sends_the_device_token_and_reads_the_quota_block() {
    mint::reset_cache();
    let (url, request) = crate::http::serve_once_capturing(
        "200 OK",
        &[],
        &serde_json::to_string(&mint_body()).expect("body"),
    );
    let response = mint::fetch(&url, "device-token").expect("fetch");
    assert_eq!(response.subs_tier_name, "Wonder");
    let captured = request.join().expect("captured");
    assert_eq!(
        crate::http::head_header(&captured.head, "authorization"),
        Some("Bearer device-token"),
        "{}",
        captured.head
    );
    assert_eq!(
        crate::http::head_header(&captured.head, "x-api-version"),
        Some("1.0.0"),
        "{}",
        captured.head
    );
    assert_eq!(captured.body, "{}");
}

#[test]
fn a_rejected_mint_login_is_signed_out_and_other_failures_fall_back() {
    mint::reset_cache();
    let root = crate::paths::scratch_dir("muse-mint-orchestration");
    let login = mint::DeviceLogin {
        token: "device-token".to_string(),
        email: Some("you@example.com".to_string()),
    };
    let serve_must_not_run = |_: &Path, _: &Path, _: &Path| -> Result<Session, ServeFailure> {
        panic!("serve must not run")
    };
    let rejected = read_current(
        &root,
        serve_must_not_run,
        |_| Some(login.clone()),
        |_| Err(mint::MintError::Unauthorized),
    );
    assert_eq!(rejected, Err(ServeFailure::SignedOut));

    let home_data = crate::paths::muse_data_for(&root);
    let mut replies = happy_replies();
    replies[1]["result"]["museHome"] = json!(home_data.to_string_lossy());
    let network = read_current(
        &root,
        |_, _, _| Ok(Script::new(replies.clone()).session(SERVE)),
        |_| Some(login.clone()),
        |_| Err(mint::MintError::Unavailable),
    )
    .expect("serve fallback");
    assert_eq!(network.reading.windows.len(), 2);

    let response: mint::MintResponse = serde_json::from_value(mint_body()).expect("parse");
    let minted = read_current(&root, serve_must_not_run, |_| Some(login), |_| Ok(response))
        .expect("mint reading");
    assert_eq!(minted.reading.plan.as_deref(), Some("Wonder"));
}
