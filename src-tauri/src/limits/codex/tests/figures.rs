//! The one gate both Codex reads ask their backend figures through (`backend_figures`): the term
//! only with the card's own access, and what it spent only on a workspace plan too.

use super::*;
use crate::accounts::model::AccessToken;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-24T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

/// An account key of this test's own: a failed read backs off per account, and must not hold
/// another test's read back.
fn account(name: &str) -> String {
    format!("test-account:{name}:{:?}", std::thread::current().id())
}

/// The signed-in login's access projection, as the read's identity check hands it over.
fn access(key: &str) -> Option<CodexAccess> {
    Some(CodexAccess {
        observation_key: key.to_string(),
        workspace_id: "team".to_string(),
        token: AccessToken::new("native-access"),
    })
}

/// Where the gate asks what was spent and the term. The usage reads are not the gate's to make.
fn urls<'a>(credit_usage: &'a str, subscriptions: &'a str) -> CodexEndpoints<'a> {
    CodexEndpoints {
        usage: "unused",
        reset_credits: "unused",
        credit_usage,
        subscriptions,
    }
}

/// A listener that never answers: a request would sit in its backlog, where `accept` finds it.
fn never_asked() -> (std::net::TcpListener, String) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/backend", listener.local_addr().unwrap());
    (listener, url)
}

fn was_asked(listener: &std::net::TcpListener) -> bool {
    listener.accept().is_ok()
}

/// A breakdown in the shape the endpoint answers, for one day's credits of one model.
fn breakdown(date: &str, credits: f64) -> String {
    json!({
        "data": [{
            "date": date,
            "product_surface_usage_values": {"cli": 1.0, "vscode": 0.0},
            "premium_usage_values": {"total_usage_credits": {}, "credit_usage_credits": {}},
            "models": [{"model": "model-0", "credits": credits}],
        }],
        "units": "credits",
        "data_freshness_ts": "2026-09-24T19:00:00Z",
        "group_by": "day",
    })
    .to_string()
}

/// A term in the shape the endpoint answers a team member, trimmed to what is read.
const TERM: &str = r#"{"id":"ws-1","entitlement":{"subscription_plan":"chatgptteamplan","expires_at":"2026-09-28T22:22:34+00:00","renews_at":"2026-09-28T16:22:34+00:00","cancels_at":null,"scheduled_plan_change":null,"is_delinquent":false},"last_active_subscription":{"will_renew":true,"cancellation_outcome":null},"plan_type":"team","active_until":"2026-09-28T16:22:34Z","will_renew":true,"cancellation_outcome":null}"#;

/// The signed-in card has no token of its own (app-server reads it), so its login's access token is
/// used, for its own workspace, and the figure is that card's.
#[test]
fn the_signed_in_workspace_card_is_asked_with_its_logins_access_token() {
    let key = account("signed-in");
    let (url, request) =
        crate::http::serve_once_capturing("200 OK", &[], &breakdown("2026-09-24", 18303.4));

    let mut card = Parsed::for_card(Some(&key), Some("self_serve_business_prolite"));
    backend_figures(
        &mut card,
        access(&key).as_ref(),
        urls(&url, &crate::http::refused_url()),
        now(),
    );
    let head = request.join().unwrap().head;

    assert!(head.contains("Bearer native-access"), "{head}");
    assert!(
        head.to_lowercase().contains("chatgpt-account-id: team"),
        "{head}"
    );
    assert_eq!(
        card.reading.credits_spent.map(|spent| spent.last_7_days),
        Some(18303.4)
    );
}

/// A personal plan pools nothing and is never asked, whatever access it is handed.
#[test]
fn a_personal_signed_in_card_is_never_asked() {
    let (listener, url) = never_asked();
    for plan in [Some("pro"), Some("plus"), None] {
        let key = account("personal");
        let mut card = Parsed::for_card(Some(&key), plan);
        backend_figures(
            &mut card,
            access(&key).as_ref(),
            urls(&url, &crate::http::refused_url()),
            now(),
        );
        assert_eq!(card.reading.credits_spent, None, "{plan:?}");
    }
    assert!(!was_asked(&listener));
}

/// Another account's token is never spent on this card, and no access at all is no figure.
#[test]
fn access_that_is_not_the_cards_account_is_not_asked() {
    let (listener, url) = never_asked();
    for handed in [access(&account("other-login")), None] {
        let mut card = Parsed::for_card(Some("the-cards-account"), Some("business"));
        backend_figures(
            &mut card,
            handed.as_ref(),
            urls(&url, &crate::http::refused_url()),
            now(),
        );
        assert_eq!(card.reading.credits_spent, None);
    }
    assert!(!was_asked(&listener));
}

#[test]
fn the_signed_in_read_asks_only_for_the_confirmed_card() {
    let term = |card: &str, access: Option<&CodexAccess>, url: &str| {
        let mut card = Parsed::for_card(Some(card), Some("pro"));
        backend_figures(&mut card, access, urls("unused", url), now());
        card.reading.subscription
    };
    renewal::forget("renewal-signed-in");
    let (listener, quiet) = never_asked();
    assert!(term("renewal-signed-in", None, &quiet).is_none());
    assert!(term("someone-else", access("renewal-signed-in").as_ref(), &quiet).is_none());
    assert!(!was_asked(&listener), "asked without a confirmed card");
    let (url, served) = crate::http::serve_once("200 OK", TERM);
    assert!(term(
        "renewal-signed-in",
        access("renewal-signed-in").as_ref(),
        &url
    )
    .is_some());
    served.join().unwrap();
    renewal::forget("renewal-signed-in");
}

/// Another account's access, or none, is no access to this card: a workspace card is asked
/// neither what it spent nor its term.
#[test]
fn a_signed_in_card_without_its_own_access_is_asked_for_no_backend_figure() {
    let (spending, credit_usage) = never_asked();
    let (terms, subscriptions) = never_asked();
    for handed in [access("another-account"), None] {
        let mut parsed = Parsed::for_card(Some("the-cards-account"), Some("business"));
        backend_figures(
            &mut parsed,
            handed.as_ref(),
            urls(&credit_usage, &subscriptions),
            chrono::Utc::now(),
        );
        assert_eq!(parsed.reading.credits_spent, None);
        assert_eq!(parsed.reading.subscription, None);
    }
    assert!(spending.accept().is_err(), "asked what the card spent");
    assert!(terms.accept().is_err(), "asked the card's term");
}

/// With its own access, a card on a personal plan, or on none, is asked its term but never what it
/// spent.
#[test]
fn a_signed_in_card_is_asked_its_term_whatever_its_plan() {
    for (plan, key) in [
        (Some("pro"), "signed-in-term-pro"),
        (None, "signed-in-term-no-plan"),
    ] {
        renewal::forget(key);
        let (spending, credit_usage) = never_asked();
        let (subscriptions, term) = crate::http::serve_once(
            "200 OK",
            r#"{"active_until":"2026-09-28T16:22:34Z","will_renew":true}"#,
        );
        let mut parsed = Parsed::for_card(Some(key), plan);
        backend_figures(
            &mut parsed,
            access(key).as_ref(),
            urls(&credit_usage, &subscriptions),
            chrono::Utc::now(),
        );
        term.join().unwrap();

        assert!(parsed.reading.subscription.is_some(), "{plan:?}");
        assert_eq!(parsed.reading.credits_spent, None, "{plan:?}");
        assert!(spending.accept().is_err(), "{plan:?}: asked what it spent");
        renewal::forget(key);
    }
}
