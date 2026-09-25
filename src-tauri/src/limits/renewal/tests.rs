use super::*;
use crate::http::{refused_url, serve_once};
use serde_json::json;
use std::time::Duration;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-25T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

/// An account's access projection, as the saved read builds it and the identity check hands it over.
fn projection(key: &str) -> CodexAccess {
    CodexAccess {
        observation_key: key.into(),
        workspace_id: "ws-1".into(),
        token: AccessToken::new("fixture-access"),
    }
}

/// A listener that never answers: a request would sit in its backlog, where `accept` finds it.
fn never_asked() -> (std::net::TcpListener, String) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/subscriptions", listener.local_addr().unwrap());
    (listener, url)
}

fn was_asked(listener: &std::net::TcpListener) -> bool {
    listener.accept().is_ok()
}

/// A term in the shape the endpoint answers a team member, trimmed to what is read.
fn term(will_renew: bool) -> Value {
    json!({
        "id": "ws-1",
        "entitlement": {
            "subscription_plan": "chatgptteamplan",
            "expires_at": "2026-09-28T22:22:34+00:00",
            "renews_at": "2026-09-28T16:22:34+00:00",
            "cancels_at": null,
            "scheduled_plan_change": null,
            "is_delinquent": false
        },
        "last_active_subscription": {"will_renew": will_renew, "cancellation_outcome": null},
        "plan_type": "team",
        "active_until": "2026-09-28T16:22:34Z",
        "will_renew": will_renew,
        "cancellation_outcome": null
    })
}

#[test]
fn reads_the_end_of_the_period_and_whether_it_renews() {
    assert_eq!(
        parse(&term(true), now()),
        Some(LimitsSubscriptionDto {
            active_until: "2026-09-28T16:22:34Z".into(),
            will_renew: true,
            note: None,
            checked_at: "2026-09-25T12:00:00Z".into(),
        })
    );
    assert!(!parse(&term(false), now()).unwrap().will_renew);
}

#[test]
fn a_cancellation_a_plan_change_and_an_overdue_payment_are_noted_in_that_order() {
    let mut cancelled = term(false);
    cancelled["cancellation_outcome"] = json!("user_cancelled");
    cancelled["entitlement"]["is_delinquent"] = json!(true);
    assert_eq!(
        parse(&cancelled, now()).unwrap().note,
        Some(SubscriptionNote::Cancelled)
    );
    let mut ending = term(false);
    ending["entitlement"]["cancels_at"] = json!("2026-09-28T16:22:34Z");
    assert_eq!(
        parse(&ending, now()).unwrap().note,
        Some(SubscriptionNote::Cancelled)
    );
    let mut overdue = term(true);
    overdue["entitlement"]["is_delinquent"] = json!(true);
    overdue["entitlement"]["scheduled_plan_change"] = json!({"plan": "chatgptplusplan"});
    assert_eq!(
        parse(&overdue, now()).unwrap().note,
        Some(SubscriptionNote::PastDue)
    );
    let mut changing = term(true);
    changing["entitlement"]["scheduled_plan_change"] = json!({"plan": "chatgptplusplan"});
    assert_eq!(
        parse(&changing, now()).unwrap().note,
        Some(SubscriptionNote::PlanChange)
    );
}

/// The entitlement's `expires_at` is not `active_until` under another name, and a flag under
/// `last_active_subscription` is not the term's: neither stands in.
#[test]
fn refuses_a_term_it_cannot_stand_behind() {
    let mut no_date = term(true);
    no_date["active_until"] = json!("soon");
    assert!(parse(&no_date, now()).is_none());
    no_date.as_object_mut().unwrap().remove("active_until");
    assert!(parse(&no_date, now()).is_none());
    let mut no_renewal = term(true);
    no_renewal["will_renew"] = json!("yes");
    assert!(parse(&no_renewal, now()).is_none());
    no_renewal.as_object_mut().unwrap().remove("will_renew");
    assert!(parse(&no_renewal, now()).is_none());
    assert!(parse(&json!({}), now()).is_none());
}

#[test]
fn asks_with_the_workspace_in_the_query_and_the_token_in_the_header() {
    let (url, served) = crate::http::serve_once_capturing("200 OK", &[], &term(false).to_string());
    let read = read(&AccessToken::new("fixture-access"), "ws 1/é", &url, now());
    let request = served.join().unwrap().head;
    assert!(!read.unwrap().will_renew);
    assert!(request.contains("account_id=ws%201%2F%C3%A9"), "{request}");
    let headers = request.to_ascii_lowercase();
    assert!(
        headers.contains("authorization: bearer fixture-access"),
        "{request}"
    );
    assert!(headers.contains("chatgpt-account-id: ws 1/é"), "{request}");
}

#[test]
fn a_refusal_is_no_term_and_is_not_asked_again_at_once() {
    let account = projection("renewal-refused");
    MEMO.forget(&account.observation_key);
    assert!(read_backed_off(&account, &refused_url(), now()).is_none());
    // Within the backoff a working endpoint is not even tried.
    let (url, served) = serve_once("200 OK", &term(true).to_string());
    assert!(read_backed_off(&account, &url, now()).is_none());
    MEMO.forget(&account.observation_key);
    assert!(read_backed_off(&account, &url, now()).is_some());
    served.join().unwrap();
}

#[test]
fn an_answer_stands_for_the_day_without_asking_again() {
    let account = projection("renewal-fresh");
    MEMO.forget(&account.observation_key);
    let (url, served) = serve_once("200 OK", &term(false).to_string());
    let first = read_backed_off(&account, &url, now()).unwrap();
    served.join().unwrap();
    // The server answered once; a second read within the day is the same term, not a request.
    assert_eq!(
        read_backed_off(&account, &refused_url(), now()),
        Some(first)
    );
    MEMO.forget(&account.observation_key);
}

/// A standing answer is served until the day is out, then the account is asked again and the new
/// answer replaces it.
#[test]
fn an_answer_stands_for_a_day_then_is_asked_again_and_replaced() {
    let account = projection("renewal-day");
    MEMO.forget(&account.observation_key);
    let (url, served) = serve_once("200 OK", &term(true).to_string());
    let first = read_backed_off(&account, &url, now()).unwrap();
    served.join().unwrap();

    MEMO.age_answer(
        &account.observation_key,
        Duration::from_secs(23 * 3600 + 59 * 60),
    );
    let (listener, quiet) = never_asked();
    assert_eq!(read_backed_off(&account, &quiet, now()), Some(first));
    assert!(!was_asked(&listener), "asked again within the day");

    MEMO.age_answer(&account.observation_key, Duration::from_secs(120));
    let (url, served) = serve_once("200 OK", &term(false).to_string());
    assert!(
        !read_backed_off(&account, &url, now()).unwrap().will_renew,
        "a day-old answer is read again and replaced"
    );
    served.join().unwrap();
    MEMO.forget(&account.observation_key);
}

/// Once a failure's wait has run out the account is asked again, and an answer clears the count.
#[test]
fn a_backoff_that_has_run_out_lets_a_working_endpoint_answer() {
    let account = projection("renewal-backoff-out");
    MEMO.forget(&account.observation_key);
    MEMO.failed_before(&account.observation_key, 3);
    let (url, served) = serve_once("200 OK", &term(true).to_string());
    assert!(read_backed_off(&account, &url, now()).is_some());
    served.join().unwrap();
    assert_eq!(MEMO.backoff_of(&account.observation_key), None);
    MEMO.forget(&account.observation_key);
}

#[test]
fn the_signed_in_read_asks_only_for_the_confirmed_card() {
    let card = |account: &str| Parsed::for_card(Some(account), Some("pro"));
    MEMO.forget("renewal-signed-in");
    let (listener, quiet) = never_asked();
    assert!(signed_in(None, &card("renewal-signed-in"), &quiet, now()).is_none());
    assert!(signed_in(
        Some(&projection("renewal-signed-in")),
        &card("someone-else"),
        &quiet,
        now()
    )
    .is_none());
    assert!(!was_asked(&listener), "asked without a confirmed card");
    let (url, served) = serve_once("200 OK", &term(true).to_string());
    assert!(signed_in(
        Some(&projection("renewal-signed-in")),
        &card("renewal-signed-in"),
        &url,
        now()
    )
    .is_some());
    served.join().unwrap();
    MEMO.forget("renewal-signed-in");
}
