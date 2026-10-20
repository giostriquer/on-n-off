use super::fixtures::{mint_body, mint_response, scoped_you};
use super::*;
use crate::dto::AgentId;
use crate::dto::{LimitWindowKind, LimitsStatus};
use crate::http::{refused_url, serve_once};
use crate::limits::outcome::no_usage;
use serde_json::Value;

fn identity() -> Identity {
    crate::accounts::muse_store::identity("You@Example.com").unwrap()
}

fn urls(url: &str) -> SavedReadUrls<'_> {
    SavedReadUrls {
        muse: url,
        ..SavedReadUrls::LIVE
    }
}

fn token() -> AccessToken {
    AccessToken::new("saved-token")
}

fn answer(email: &str, usage: Value) -> String {
    let mut body = mint_body();
    body["user_email"] = json!(email);
    body["subs_usage"] = usage;
    body.to_string()
}

fn usage() -> Value {
    mint_body()["subs_usage"].clone()
}

#[test]
fn a_saved_login_reads_its_own_card_under_its_scoped_id() {
    let (url, served) = serve_once("200 OK", &answer("You@Example.com", usage()));

    let card = read_saved(&identity(), &token(), &urls(&url)).unwrap();
    served.join().unwrap();

    assert_eq!(card.status, LimitsStatus::Ok);
    assert!(!card.current_account);
    let account = card.account.unwrap();
    assert_eq!(account.id, identity().observation_key());
    assert_eq!(account.legacy_id.as_deref(), Some("email:you@example.com"));
    assert_eq!(card.reading.plan.as_deref(), Some("Wonder"));
    assert_eq!(
        card.reading
            .windows
            .iter()
            .map(|window| window.kind)
            .collect::<Vec<_>>(),
        [LimitWindowKind::Weekly, LimitWindowKind::Session]
    );
}

#[test]
fn an_answer_for_another_email_is_another_account() {
    let (url, served) = serve_once("200 OK", &answer("someone@example.com", usage()));

    let read = read_saved(&identity(), &token(), &urls(&url));
    served.join().unwrap();

    assert_eq!(read.err(), Some(ReadFailure::OtherAccount));
}

#[test]
fn a_rejected_token_is_a_refused_login() {
    let (url, served) = serve_once("401 Unauthorized", r#"{"title":"Authentication Error"}"#);

    let read = read_saved(&identity(), &token(), &urls(&url));
    served.join().unwrap();

    assert_eq!(read.err(), Some(ReadFailure::LoginRefused));
}

#[test]
fn an_idle_window_reads_its_plan_and_an_unused_session() {
    let (url, served) = serve_once(
        "200 OK",
        &answer("you@example.com", serde_json::Value::Null),
    );

    let card = read_saved(&identity(), &token(), &urls(&url)).unwrap();
    served.join().unwrap();

    assert_eq!(card.status, LimitsStatus::Ok);
    assert_eq!(
        card.account.map(|account| account.id),
        Some(identity().observation_key())
    );
    assert_eq!(card.reading.plan.as_deref(), Some("Wonder"));
    assert_eq!(
        card.reading
            .windows
            .iter()
            .map(|window| (window.kind, window.used_percent, window.resets_at.clone()))
            .collect::<Vec<_>>(),
        [(LimitWindowKind::Session, 0.0, None)]
    );
}

#[test]
fn an_inactive_subscription_is_unsupported_and_an_unreadable_usage_block_no_usage() {
    let inactive = json!({"is_subs_active": false, "user_email": "you@example.com"}).to_string();
    let unreadable = answer("you@example.com", json!({"window": null, "weekly": null}));
    for (body, expected) in [
        (inactive, ReadFailure::Unsupported(Unsupported::Inactive)),
        (unreadable, no_usage(AgentId::Muse)),
    ] {
        let (url, served) = serve_once("200 OK", &body);
        let read = read_saved(&identity(), &token(), &urls(&url));
        served.join().unwrap();
        assert_eq!(read.err(), Some(expected), "{body}");
    }
}

#[test]
fn an_unreachable_endpoint_is_unavailable() {
    assert!(matches!(
        read_saved(&identity(), &token(), &urls(&refused_url())),
        Err(ReadFailure::RequestFailed(_))
    ));
}

fn normalized(body: Value) -> Result<Parsed, ReadFailure> {
    super::answer(Some("You@Example.com".to_string()), &mint_response(body))
}

fn mint_reading(body: Value) -> Parsed {
    normalized(body).expect("mint reading")
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
fn a_mint_response_normalizes_to_a_card_with_both_windows() {
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
fn a_usage_outside_zero_to_a_hundred_percent_is_clamped() {
    let mut body = mint_body();
    body["subs_usage"]["weekly"]["used_percent"] = json!(140);
    body["subs_usage"]["window"]["used_percent"] = json!(-3);
    let parsed = mint_reading(body);
    assert_eq!(
        window_of(&parsed, LimitWindowKind::Weekly).used_percent,
        100.0
    );
    assert_eq!(
        window_of(&parsed, LimitWindowKind::Session).used_percent,
        0.0
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
fn an_active_subscription_without_a_usage_block_reads_as_an_idle_window() {
    let idle = json!({"is_subs_active": true, "subs_tier_name": "Wonder", "subs_usage": null});
    let parsed = mint_reading(idle);
    assert_eq!(
        parsed.account.as_ref().map(|account| account.id.as_str()),
        Some(scoped_you().as_str())
    );
    assert_eq!(parsed.reading.plan.as_deref(), Some("Wonder"));
    assert_eq!(parsed.reading.windows.len(), 1);
    let session = window_of(&parsed, LimitWindowKind::Session);
    assert_eq!(session.id, "session");
    assert_eq!(session.label, "5 hour · all models");
    assert_eq!(session.used_percent, 0.0);
    assert_eq!(session.resets_at, None);
    assert_eq!(session.window_seconds, Some(18_000));

    let unnamed = json!({"is_subs_active": true, "subs_tier_name": "27681631238169137"});
    assert_eq!(mint_reading(unnamed).reading.plan, None);
}

#[test]
fn a_mint_answer_with_no_usable_window_answers_no_window() {
    let no_window = |body: Value| mint_reading(body).reading.windows.is_empty();
    for unconfirmed in [
        json!({"subs_tier_name": "Wonder", "subs_usage": null}),
        json!({"is_subs_active": null, "subs_tier_name": "Wonder"}),
    ] {
        assert!(no_window(unconfirmed.clone()), "{unconfirmed}");
    }

    let mut empty = mint_body();
    empty["subs_usage"]["window"] = json!(null);
    empty["subs_usage"]["weekly"] = json!(null);
    assert!(no_window(empty));

    let mut unusable = mint_body();
    unusable["subs_usage"]["window"] = json!({"window_duration_mins": 300});
    unusable["subs_usage"]["weekly"] = json!({"used_percent": null, "resets_at": 1_760_500_000i64});
    assert!(no_window(unusable));

    let mut weekly_only = mint_body();
    weekly_only["subs_usage"]["window"] = json!(null);
    let parsed = mint_reading(weekly_only);
    assert_eq!(parsed.reading.windows.len(), 1);
    assert_eq!(parsed.reading.windows[0].id, "weekly");
}

#[test]
fn an_inactive_or_unpaid_subscription_reads_unsupported() {
    assert_eq!(
        normalized(json!({"is_subs_active": false, "subs_tier_name": "Wonder"})).err(),
        Some(ReadFailure::Unsupported(Unsupported::Inactive))
    );
    assert_eq!(
        normalized(json!({"is_subs_active": true, "require_payment": true})).err(),
        Some(ReadFailure::Unsupported(Unsupported::BillingIncomplete))
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
    let response = fetch(&url, &AccessToken::new("device-token")).expect("fetch");
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
    assert!(super::answer(None, &response).is_ok());
}

#[test]
fn a_failed_mint_call_names_its_failure() {
    for (status, body, expected) in [
        (
            "401 Unauthorized",
            r#"{"title":"Authentication Error"}"#,
            "token rejected",
        ),
        ("403 Forbidden", "{}", "HTTP 403"),
        ("429 Too Many Requests", "{}", "rate limited"),
        ("500 Internal Server Error", "{}", "HTTP 500"),
        ("200 OK", "not json", "unreadable response"),
        (
            "200 OK",
            r#"{"subs_usage": "wrong shape"}"#,
            "unreadable response: Unreadable Muse key answer.",
        ),
    ] {
        let (url, server) = crate::http::serve_once(status, body);
        let error = fetch(&url, &AccessToken::new("device-token"))
            .expect_err("a failed mint call")
            .to_string();
        assert!(error.starts_with(expected), "{status} {body}: {error}");
        server.join().expect("served");
    }
    let refused = fetch(
        &crate::http::refused_url(),
        &AccessToken::new("device-token"),
    );
    assert!(matches!(refused, Err(HttpError::Network(_))), "{refused:?}");
}
