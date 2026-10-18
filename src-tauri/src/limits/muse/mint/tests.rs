use super::*;
use crate::dto::{LimitWindowKind, LimitsStatus};
use crate::http::{refused_url, serve_once};

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

fn answer(email: &str, usage: serde_json::Value) -> String {
    json!({
        "subs_tier_name": "Muse Code Power Usage",
        "is_subs_active": true,
        "require_payment": false,
        "user_email": email,
        "subs_usage": usage,
    })
    .to_string()
}

fn usage() -> serde_json::Value {
    json!({
        "tier": "power",
        "window": {"used_percent": 41.0, "window_duration_mins": 300.0, "resets_at": 1_787_000_000.0},
        "weekly": {"used_percent": 22.0, "resets_at": 1_787_500_000.0},
    })
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
    assert_eq!(card.reading.plan.as_deref(), Some("Muse Code Power Usage"));
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
    assert_eq!(card.reading.plan.as_deref(), Some("Muse Code Power Usage"));
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
