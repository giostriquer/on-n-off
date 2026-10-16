use super::*;
use crate::accounts::model::AccessToken;
use crate::accounts::muse_store::MuseLogin;
use crate::dto::LimitWindowDto;
use crate::dto::{LimitWindowKind, LimitsStatus};
use crate::json_line_session::Script;
use serde_json::{json, Value};
use std::path::PathBuf;

fn scoped_you() -> String {
    muse_identity("you@example.com").observation_key()
}

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
    let parsed = normalize(answers, None).expect("normalize");

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
    assert_eq!(account.id, scoped_you());
    assert_eq!(account.legacy_id.as_deref(), Some("email:you@example.com"));
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
    let parsed = normalize(answers, None).expect("normalize");
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
        let card = finish_read(normalize(answers, None));
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
    let card = finish_read(normalize(answers, None));
    assert_eq!(card.status, LimitsStatus::Failed);
    assert_eq!(
        card.account.as_ref().map(|account| account.id.clone()),
        Some(scoped_you())
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
    let card = finish_read(read_with(
        &root,
        |_, _, _| begin(SERVE, Command::new(&missing)),
        None,
    ));

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

fn signed_in(email: Option<&str>) -> Option<MuseLogin> {
    Some(MuseLogin {
        email: email.map(str::to_string),
        token: Some(AccessToken::new("device-token")),
    })
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

fn mint_response(body: Value) -> mint::MintResponse {
    serde_json::from_value(body).expect("mint response")
}

fn normalized(body: Value) -> Option<Result<Parsed, ServeFailure>> {
    mint::normalize(Some("You@Example.com".to_string()), &mint_response(body))
}

fn mint_reading(body: Value) -> Parsed {
    normalized(body).expect("mint reading").expect("parsed")
}

fn window_of(parsed: &Parsed, kind: LimitWindowKind) -> &LimitWindowDto {
    parsed
        .reading
        .windows
        .iter()
        .find(|window| window.kind == kind)
        .expect("window")
}

#[test]
fn a_mint_response_normalizes_to_the_same_card_as_serve() {
    let parsed = mint_reading(mint_body());
    let account = parsed.account.as_ref().expect("account");
    assert_eq!(account.id, scoped_you());
    assert_eq!(account.legacy_id.as_deref(), Some("email:you@example.com"));
    assert_eq!(account.label.as_deref(), Some("You@Example.com"));
    assert_eq!(parsed.reading.plan.as_deref(), Some("Wonder"));
    assert_eq!(parsed.reading.windows.len(), 2);

    let weekly = window_of(&parsed, LimitWindowKind::Weekly);
    assert_eq!(weekly.id, "weekly");
    assert_eq!(weekly.label, "Weekly · all models");
    assert_eq!(weekly.window_seconds, None);
    assert_eq!(weekly.used_percent, 22.0);
    assert_eq!(
        weekly.resets_at.as_deref(),
        Some("2025-10-15T03:46:40.000Z")
    );

    let session = window_of(&parsed, LimitWindowKind::Session);
    assert_eq!(session.id, "session");
    assert_eq!(session.label, "5 hour · all models");
    assert_eq!(session.used_percent, 41.0);
    assert_eq!(session.window_seconds, Some(18_000));
    assert_eq!(
        session.resets_at.as_deref(),
        Some("2025-10-09T08:53:20.000Z")
    );
}

#[test]
fn a_session_window_without_a_duration_is_the_five_hour_window() {
    for duration in [json!(null), json!(0), json!(-5)] {
        let mut body = mint_body();
        body["subs_usage"]["window"]["window_duration_mins"] = duration.clone();
        let parsed = mint_reading(body);
        let session = window_of(&parsed, LimitWindowKind::Session);
        assert_eq!(session.label, "5 hour · all models", "{duration}");
        assert_eq!(session.window_seconds, Some(18_000), "{duration}");
    }
    let mut body = mint_body();
    body["subs_usage"]["window"]["window_duration_mins"] = json!(120);
    let parsed = mint_reading(body);
    let session = window_of(&parsed, LimitWindowKind::Session);
    assert_eq!(session.label, "2 hour · all models");
    assert_eq!(session.window_seconds, Some(7_200));
}

#[test]
fn the_plan_is_the_tier_name_else_a_tier_that_reads_as_a_name() {
    let mut body = mint_body();
    body["subs_tier_name"] = json!(" ");
    body["subs_usage"]["tier"] = json!("wonder");
    assert_eq!(mint_reading(body).reading.plan.as_deref(), Some("wonder"));

    let mut body = mint_body();
    body["subs_tier_name"] = json!("");
    assert_eq!(mint_reading(body).reading.plan, None);
}

#[test]
fn a_mint_answer_with_no_usable_window_falls_back() {
    let idle = json!({"is_subs_active": true, "subs_tier_name": "Wonder", "subs_usage": null});
    assert!(normalized(idle).is_none());

    let mut empty = mint_body();
    empty["subs_usage"]["window"] = json!(null);
    empty["subs_usage"]["weekly"] = json!(null);
    assert!(normalized(empty).is_none());

    let mut unusable = mint_body();
    unusable["subs_usage"]["window"] = json!({"window_duration_mins": 300});
    unusable["subs_usage"]["weekly"] = json!({"used_percent": null, "resets_at": 1_760_500_000i64});
    assert!(normalized(unusable).is_none());

    let mut weekly_only = mint_body();
    weekly_only["subs_usage"]["window"] = json!(null);
    let parsed = mint_reading(weekly_only);
    assert_eq!(parsed.reading.windows.len(), 1);
    assert_eq!(parsed.reading.windows[0].id, "weekly");
}

#[test]
fn an_inactive_or_unpaid_subscription_reads_unsupported() {
    let inactive = normalized(json!({"is_subs_active": false, "subs_tier_name": "Wonder"}));
    assert!(
        matches!(&inactive, Some(Err(ServeFailure::Unsupported(message))) if message.contains("inactive")),
        "{inactive:?}"
    );
    let billing = normalized(json!({"is_subs_active": true, "require_payment": true}));
    assert!(
        matches!(&billing, Some(Err(ServeFailure::Unsupported(message))) if message.contains("billing")),
        "{billing:?}"
    );
}

#[test]
fn a_minted_key_in_the_response_is_never_kept() {
    let mut body = mint_body();
    body["api_key"] = json!("LLM|secret-minted-key");
    body["account"] = json!({"id": "acct-1"});
    let debug = format!("{:?}", mint_response(body));
    assert!(!debug.contains("secret-minted-key"), "{debug}");
    assert!(!debug.contains("acct-1"), "{debug}");
}

#[test]
fn the_mint_call_sends_the_device_token_and_reads_the_quota_block() {
    let (url, request) = crate::http::serve_once_capturing(
        "200 OK",
        &[],
        &serde_json::to_string(&mint_body()).expect("body"),
    );
    let response = mint::fetch(&url, &AccessToken::new("device-token")).expect("fetch");
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
    assert!(mint::normalize(None, &response).is_some());
}

#[test]
fn a_failed_mint_call_gives_no_answer() {
    for (status, body) in [
        ("401 Unauthorized", r#"{"title":"Authentication Error"}"#),
        ("403 Forbidden", "{}"),
        ("429 Too Many Requests", "{}"),
        ("500 Internal Server Error", "{}"),
        ("200 OK", "not json"),
        ("200 OK", r#"{"subs_usage": "wrong shape"}"#),
    ] {
        let (url, server) = crate::http::serve_once(status, body);
        assert!(
            mint::fetch(&url, &AccessToken::new("device-token")).is_none(),
            "{status} {body}"
        );
        server.join().expect("served");
    }
    assert!(mint::fetch(
        &crate::http::refused_url(),
        &AccessToken::new("device-token")
    )
    .is_none());
}

fn serve_read(root: &Path) -> impl FnOnce(&Path, &Path, &Path) -> Result<Session, ServeFailure> {
    let mut replies = happy_replies();
    replies[1]["result"]["museHome"] = json!(crate::paths::muse_data_for(root).to_string_lossy());
    move |_, _, _| Ok(Script::new(replies).session(SERVE))
}

fn serve_must_not_run(_: &Path, _: &Path, _: &Path) -> Result<Session, ServeFailure> {
    panic!("serve must not run")
}

#[test]
fn a_mint_reading_answers_without_starting_serve() {
    let root = crate::paths::scratch_dir("muse-mint-reading");
    let config = crate::paths::muse_config_for(&root);
    let read = read_current(
        &root,
        serve_must_not_run,
        |asked: &Path| {
            assert_eq!(asked, config);
            signed_in(Some("you@example.com"))
        },
        |token| {
            assert_eq!(token.authorization(), "Bearer device-token");
            Some(mint_response(mint_body()))
        },
    )
    .expect("mint reading");
    assert_eq!(read.reading.plan.as_deref(), Some("Wonder"));
}

#[test]
fn every_mint_miss_falls_back_to_the_serve_read() {
    let root = crate::paths::scratch_dir("muse-mint-fallback");
    let serve_windows = |read: Result<Parsed, ServeFailure>| {
        let read = read.expect("serve fallback");
        assert_eq!(read.reading.plan.as_deref(), Some("wonder"));
        read.reading.windows.len()
    };

    let no_login = read_current(
        &root,
        serve_read(&root),
        |_| None,
        |_| panic!("no login means no mint"),
    );
    assert_eq!(serve_windows(no_login), 2);

    let expired = read_current(
        &root,
        serve_read(&root),
        |_| {
            Some(MuseLogin {
                email: Some("you@example.com".to_string()),
                token: None,
            })
        },
        |_| panic!("no token means no mint"),
    );
    assert_eq!(serve_windows(expired), 2);

    let rejected = read_current(
        &root,
        serve_read(&root),
        |_| signed_in(Some("you@example.com")),
        |_| None,
    );
    assert_eq!(serve_windows(rejected), 2);

    let idle = read_current(
        &root,
        serve_read(&root),
        |_| signed_in(Some("you@example.com")),
        |_| {
            Some(mint_response(
                json!({"is_subs_active": true, "subs_usage": null}),
            ))
        },
    );
    assert_eq!(serve_windows(idle), 2);
}

#[test]
fn a_serve_fallback_names_the_account_as_the_mint_reading_does() {
    let root = crate::paths::scratch_dir("muse-identity-fallback");
    let minted = read_current(
        &root,
        serve_must_not_run,
        |_| signed_in(Some("You@Example.com")),
        |_| Some(mint_response(mint_body())),
    )
    .expect("mint reading")
    .account
    .expect("account");
    assert_eq!(minted.id, scoped_you());
    for serve_label in ["Display Name", "Other@Example.org"] {
        let mut replies = happy_replies();
        replies[1]["result"]["museHome"] =
            json!(crate::paths::muse_data_for(&root).to_string_lossy());
        replies[2]["result"]["label"] = json!(serve_label);
        replies[4]["result"]["label"] = json!(serve_label);
        let fallback = read_current(
            &root,
            move |_, _, _| Ok(Script::new(replies).session(SERVE)),
            |_| signed_in(Some("You@Example.com")),
            |_| None,
        )
        .expect("serve fallback")
        .account
        .expect("account");
        assert_eq!(fallback, minted, "{serve_label}");
    }
}

#[test]
fn a_login_without_an_email_is_the_default_account_on_both_paths() {
    let root = crate::paths::scratch_dir("muse-identity-default");
    let minted = read_current(
        &root,
        serve_must_not_run,
        |_| signed_in(None),
        |_| Some(mint_response(mint_body())),
    )
    .expect("mint reading")
    .account
    .expect("account");
    let fallback = read_current(&root, serve_read(&root), |_| signed_in(None), |_| None)
        .expect("serve fallback")
        .account
        .expect("account");
    assert_eq!(minted.id, super::super::DEFAULT_ACCOUNT);
    assert_eq!(fallback, minted);
}

#[test]
fn the_serve_label_names_the_account_only_without_an_auth_login() {
    let root = crate::paths::scratch_dir("muse-identity-serve-label");
    let read = read_current(
        &root,
        serve_read(&root),
        |_| None,
        |_| panic!("no login means no mint"),
    )
    .expect("serve read");
    let account = read.account.expect("account");
    assert_eq!(account.id, scoped_you());
    assert_eq!(account.legacy_id.as_deref(), Some("email:you@example.com"));
    assert_eq!(account.label.as_deref(), Some("You@Example.com"));
}
