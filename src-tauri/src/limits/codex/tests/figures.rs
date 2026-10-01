use super::*;
use crate::accounts::model::AccessToken;
use crate::http::{never_asked, was_asked};

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-24T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn account(name: &str) -> String {
    format!("test-account:{name}:{:?}", std::thread::current().id())
}

fn access(key: &str) -> Option<CodexAccess> {
    Some(CodexAccess {
        observation_key: key.to_string(),
        workspace_id: "team".to_string(),
        token: AccessToken::new("native-access"),
    })
}

fn urls<'a>(credit_usage: &'a str, subscriptions: &'a str) -> CodexEndpoints<'a> {
    CodexEndpoints {
        usage: "unused",
        reset_credits: "unused",
        credit_usage,
        subscriptions,
    }
}

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

const TERM: &str = r#"{"id":"ws-1","entitlement":{"subscription_plan":"chatgptteamplan","expires_at":"2026-09-28T22:22:34+00:00","renews_at":"2026-09-28T16:22:34+00:00","cancels_at":null,"scheduled_plan_change":null,"is_delinquent":false},"last_active_subscription":{"will_renew":true,"cancellation_outcome":null},"plan_type":"team","active_until":"2026-09-28T16:22:34Z","will_renew":true,"cancellation_outcome":null}"#;

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

#[derive(Debug, Clone, Copy, PartialEq)]
enum Handed {
    Own,
    Other,
    None,
}

enum Endpoint {
    Asked(String, std::thread::JoinHandle<String>),
    NeverAsked(std::net::TcpListener, String),
}

impl Endpoint {
    fn new(asked: bool, body: &str) -> Self {
        if asked {
            let (url, request) = crate::http::serve_once("200 OK", body);
            Self::Asked(url, request)
        } else {
            let (listener, url) = never_asked();
            Self::NeverAsked(listener, url)
        }
    }

    fn url(&self) -> &str {
        match self {
            Self::Asked(url, _) | Self::NeverAsked(_, url) => url,
        }
    }

    fn check(self, case: &str) {
        match self {
            Self::Asked(_, request) => {
                request.join().unwrap();
            }
            Self::NeverAsked(listener, _) => assert!(!was_asked(&listener), "{case}: asked"),
        }
    }
}

#[test]
fn each_figure_is_asked_only_with_the_cards_own_access_and_spending_only_on_a_workspace_plan() {
    let plans = [
        Some("business"),
        Some("self_serve_business_prolite"),
        Some("pro"),
        Some("plus"),
        None,
    ];
    for plan in plans {
        for handed in [Handed::Own, Handed::Other, Handed::None] {
            let case = format!("{plan:?} with {handed:?} access");
            let key = account(&format!("table-{plan:?}-{handed:?}"));
            let access = match handed {
                Handed::Own => access(&key),
                Handed::Other => access(&account("another-account")),
                Handed::None => None,
            };
            let asks_term = handed == Handed::Own;
            let workspace = matches!(plan, Some("business" | "self_serve_business_prolite"));
            let asks_spending = asks_term && workspace;
            let spending = Endpoint::new(asks_spending, &breakdown("2026-09-24", 5.5));
            let term = Endpoint::new(asks_term, TERM);

            let mut card = Parsed::for_card(Some(&key), plan);
            backend_figures(
                &mut card,
                access.as_ref(),
                urls(spending.url(), term.url()),
                now(),
            );
            spending.check(&case);
            term.check(&case);

            assert_eq!(
                card.reading.credits_spent.is_some(),
                asks_spending,
                "{case}: what was spent"
            );
            assert_eq!(
                card.reading.subscription.is_some(),
                asks_term,
                "{case}: the term"
            );
        }
    }
}
