use super::*;
use crate::dto::{LimitWindowKind, LimitsStatus};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::PathBuf;

struct FakeTransport {
    received: VecDeque<Value>,
    sent: Vec<Value>,
}

impl JsonLineTransport for FakeTransport {
    fn send(&mut self, message: &Value) -> Result<(), TransportError> {
        self.sent.push(message.clone());
        Ok(())
    }

    fn receive(&mut self) -> Result<Value, TransportError> {
        self.received.pop_front().ok_or_else(|| TransportError {
            kind: QueryErrorKind::Other,
            message: "no more serve messages".to_string(),
        })
    }
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

fn happy_transport() -> FakeTransport {
    FakeTransport {
        received: VecDeque::from([
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
        ]),
        sent: Vec::new(),
    }
}

#[test]
fn handshake_requests_the_experimental_api_and_reads_usage() {
    let mut transport = happy_transport();
    let session = query_serve(&data_home(), &mut transport).expect("query");
    let parsed = normalize(session).expect("normalize");

    assert_eq!(
        transport.sent[0],
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "clientInfo": {"name": "on_n_off", "title": "on-n-off", "version": env!("CARGO_PKG_VERSION")},
            "capabilities": {"experimentalApi": true, "userInputDialogs": false}
        }})
    );
    let methods: Vec<_> = transport
        .sent
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
    let mut transport = happy_transport();
    transport.received[3]["result"]["usage"]["tier"] = json!("27681631238169137");
    let session = query_serve(&data_home(), &mut transport).expect("query");
    let parsed = normalize(session).expect("normalize");
    assert_eq!(parsed.reading.plan, None);
    assert_eq!(parsed.reading.windows.len(), 2, "windows still read");
}

#[test]
fn a_different_muse_home_fails_the_read() {
    let mut transport = FakeTransport {
        received: VecDeque::from([json!({"id": 1, "result": {
            "museHome": "/somewhere/else/muse",
            "experimentalApi": true
        }})]),
        sent: Vec::new(),
    };
    let error = query_serve(&data_home(), &mut transport).expect_err("home mismatch");
    assert!(error.message.contains("expected"), "{error:?}");
}

#[test]
fn a_host_without_the_experimental_api_fails_with_update_guidance() {
    let mut transport = FakeTransport {
        received: VecDeque::from([json!({"id": 1, "result": {
            "museHome": "/fixture/.local/share/muse",
            "experimentalApi": false
        }})]),
        sent: Vec::new(),
    };
    let error = query_serve(&data_home(), &mut transport).expect_err("no experimental api");
    assert!(error.message.contains("Update Muse"), "{error:?}");
}

#[test]
fn signed_out_and_api_key_states_map_to_their_cards() {
    for (state, status, needle) in [
        ("loggedOut", LimitsStatus::SignedOut, "Sign in with `muse`"),
        ("apiKey", LimitsStatus::Unsupported, "API key"),
        ("envKey", LimitsStatus::Unsupported, "API key"),
    ] {
        let mut transport = FakeTransport {
            received: VecDeque::from([
                initialized(),
                json!({"id": 2, "result": account(state, None)}),
            ]),
            sent: Vec::new(),
        };
        let session = query_serve(&data_home(), &mut transport).expect("query");
        let card = finish_read(normalize(session));
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
    let mut transport = FakeTransport {
        received: VecDeque::from([
            initialized(),
            json!({"id": 2, "result": account("accountLogin", Some("you@example.com"))}),
            json!({"id": 3, "result": {}}),
            json!({"id": 4, "result": account("accountLogin", Some("you@example.com"))}),
        ]),
        sent: Vec::new(),
    };
    let session = query_serve(&data_home(), &mut transport).expect("query");
    let card = finish_read(normalize(session));
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
    let mut transport = happy_transport();
    let last = transport.received.len() - 1;
    transport.received[last] =
        json!({"id": 4, "result": account("accountLogin", Some("other@example.com"))});
    let error = query_serve(&data_home(), &mut transport).expect_err("changed account");
    assert!(error.message.contains("changed"), "{error:?}");
}

#[test]
fn a_missing_usage_method_asks_for_an_update() {
    let mut transport = FakeTransport {
        received: VecDeque::from([
            initialized(),
            json!({"id": 2, "result": account("accountLogin", Some("you@example.com"))}),
            json!({"id": 3, "error": {"code": -32601, "message": "method not found"}}),
        ]),
        sent: Vec::new(),
    };
    let error = query_serve(&data_home(), &mut transport).expect_err("method missing");
    assert!(error.message.contains("Update Muse"), "{error:?}");
    assert!(error.message.contains("usage/read"), "{error:?}");
}
