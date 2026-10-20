use super::fixtures::{mint_body, mint_response, scoped_you};
use super::*;
use crate::dto::{LimitWindowKind, LimitsStatus};
use crate::http::{refused_url, serve_once};
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

    assert!(matches!(read, Err(SavedReadError::OtherAccount)));
}

#[test]
fn a_rejected_token_reads_as_unauthorized() {
    let (url, served) = serve_once("401 Unauthorized", r#"{"title":"Authentication Error"}"#);

    let read = read_saved(&identity(), &token(), &urls(&url));
    served.join().unwrap();

    assert!(matches!(
        read,
        Err(SavedReadError::Http(HttpError::Unauthorized))
    ));
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
fn an_inactive_subscription_and_an_unreadable_usage_block_are_unavailable_rather_than_failed() {
    let inactive = json!({"is_subs_active": false, "user_email": "you@example.com"}).to_string();
    let unreadable = answer("you@example.com", json!({"window": null, "weekly": null}));
    for body in [inactive, unreadable] {
        let (url, served) = serve_once("200 OK", &body);
        let read = read_saved(&identity(), &token(), &urls(&url));
        served.join().unwrap();
        assert!(
            matches!(read, Err(SavedReadError::Unavailable(_))),
            "{body}"
        );
    }
}

#[test]
fn an_unreachable_endpoint_is_a_network_failure() {
    assert!(matches!(
        read_saved(&identity(), &token(), &urls(&refused_url())),
        Err(SavedReadError::Http(HttpError::Network(_)))
    ));
}

fn normalized(body: Value) -> Option<Result<Parsed, ReadFailure>> {
    normalize(Some("You@Example.com".to_string()), &mint_response(body))
}

fn mint_reading(body: Value) -> Parsed {
    normalized(body).expect("mint reading").expect("parsed")
}

fn facts(body: Value) -> Vec<QuotaWindow> {
    quota_windows(&mint_response(body)).expect("quota windows")
}

fn session(facts: &[QuotaWindow]) -> &QuotaWindow {
    facts
        .iter()
        .find(|window| window.id == "session")
        .expect("session window")
}

#[test]
fn a_mint_response_maps_to_its_account_plan_and_both_windows() {
    let parsed = mint_reading(mint_body());
    let account = parsed.account.as_ref().expect("account");
    assert_eq!(account.id, scoped_you());
    assert_eq!(account.legacy_id.as_deref(), Some("email:you@example.com"));
    assert_eq!(account.label.as_deref(), Some("You@Example.com"));
    assert_eq!(parsed.reading.plan.as_deref(), Some("Wonder"));
    assert_eq!(parsed.reading.windows.len(), 2);

    assert_eq!(
        facts(mint_body()),
        [
            QuotaWindow {
                id: "weekly".into(),
                scope: Scope::AllModels,
                length: Some(ONE_WEEK),
                used_percent: 22.0,
                reset: DateTime::from_timestamp(1_760_500_000, 0).map(ResetInstant::from),
            },
            QuotaWindow {
                id: "session".into(),
                scope: Scope::AllModels,
                length: Some(FIVE_HOURS),
                used_percent: 41.0,
                reset: DateTime::from_timestamp(1_760_000_000, 0).map(ResetInstant::from),
            },
        ]
    );
}

#[test]
fn a_usage_and_a_reset_are_handed_on_as_meta_reports_them() {
    let mut body = mint_body();
    body["subs_usage"]["weekly"]["used_percent"] = json!(140);
    body["subs_usage"]["window"]["used_percent"] = json!(-3);
    body["subs_usage"]["window"]["resets_at"] = json!(1_760_000_000.5);
    body["subs_usage"]["weekly"]["resets_at"] = json!(-1);
    let facts = facts(body);
    let used: Vec<f64> = facts.iter().map(|window| window.used_percent).collect();
    assert_eq!(used, [140.0, -3.0]);
    assert_eq!(
        session(&facts).reset,
        DateTime::from_timestamp_millis(1_760_000_000_500).map(ResetInstant::from)
    );
    assert_eq!(facts[0].reset, None);
}

#[test]
fn a_reset_at_or_before_the_epoch_is_no_reset() {
    for stamp in [json!(0), json!(0.0), json!(-1)] {
        let mut body = mint_body();
        body["subs_usage"]["weekly"]["resets_at"] = stamp.clone();
        body["subs_usage"]["window"]["resets_at"] = stamp.clone();
        let resets: Vec<Option<ResetInstant>> =
            facts(body).iter().map(|window| window.reset).collect();
        assert_eq!(resets, [None, None], "{stamp}");
    }
}

#[test]
fn a_session_window_without_a_duration_is_the_five_hour_window() {
    for duration in [json!(null), json!(0), json!(-5)] {
        let mut body = mint_body();
        body["subs_usage"]["window"]["window_duration_mins"] = duration.clone();
        assert_eq!(session(&facts(body)).length, Some(FIVE_HOURS), "{duration}");
    }
    let mut body = mint_body();
    body["subs_usage"]["window"]["window_duration_mins"] = json!(120);
    assert_eq!(
        session(&facts(body)).length,
        Some(Duration::from_secs(7_200))
    );
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
    let parsed = mint_reading(idle.clone());
    assert_eq!(
        parsed.account.as_ref().map(|account| account.id.as_str()),
        Some(scoped_you().as_str())
    );
    assert_eq!(parsed.reading.plan.as_deref(), Some("Wonder"));
    assert_eq!(facts(idle), [QuotaWindow::idle("session", FIVE_HOURS)]);

    let unnamed = json!({"is_subs_active": true, "subs_tier_name": "27681631238169137"});
    assert_eq!(mint_reading(unnamed).reading.plan, None);
}

#[test]
fn a_mint_answer_with_no_usable_window_is_no_reading() {
    for unconfirmed in [
        json!({"subs_tier_name": "Wonder", "subs_usage": null}),
        json!({"is_subs_active": null, "subs_tier_name": "Wonder"}),
    ] {
        assert!(normalized(unconfirmed.clone()).is_none(), "{unconfirmed}");
    }

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
        matches!(&inactive, Some(Err(ReadFailure::Unsupported(message))) if message.contains("inactive")),
        "{inactive:?}"
    );
    let billing = normalized(json!({"is_subs_active": true, "require_payment": true}));
    assert!(
        matches!(&billing, Some(Err(ReadFailure::Unsupported(message))) if message.contains("billing")),
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
    assert!(normalize(None, &response).is_some());
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
