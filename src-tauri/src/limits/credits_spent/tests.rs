use super::*;
use crate::limits::backend_memo;
use serde_json::json;
use std::time::Instant;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-24T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn today() -> NaiveDate {
    now().date_naive()
}

/// A breakdown in the shape the endpoint answers: one row per day, with the credits each model used.
fn breakdown(days: &[(&str, &[f64])]) -> Value {
    json!({
        "data": days.iter().map(|(date, credits)| json!({
            "date": date,
            "product_surface_usage_values": {"cli": 1.0, "vscode": 0.0},
            "premium_usage_values": {"total_usage_credits": {}, "credit_usage_credits": {}},
            "models": credits.iter().enumerate().map(|(i, credits)| json!({
                "model": format!("model-{i}"),
                "credits": credits,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "units": "credits",
        "data_freshness_ts": "2026-09-24T19:00:00Z",
        "group_by": "day",
    })
}

/// Codex's own parser sums each day's per-model credits, and the days in the window.
#[test]
fn sums_each_days_models_over_the_last_7_and_30_days() {
    let payload = breakdown(&[
        ("2026-08-26", &[100.0]),
        ("2026-09-17", &[50.0, 25.5]),
        ("2026-09-18", &[1000.0]),
        ("2026-09-24T00:00:00Z", &[200.25, 0.25]),
    ]);

    assert_eq!(
        parse(&payload, now()),
        Some(LimitsCreditsSpentDto {
            last_7_days: 1200.5,
            last_30_days: 1376.0,
            updated_at: Some("2026-09-24T19:00:00Z".to_string()),
        })
    );
}

#[test]
fn days_outside_the_30_day_window_are_left_out() {
    let payload = breakdown(&[
        ("2026-08-25", &[999.0]),
        ("2026-08-26", &[1.0]),
        ("2026-09-25", &[999.0]),
    ]);

    let spent = parse(&payload, now()).unwrap();
    assert_eq!((spent.last_7_days, spent.last_30_days), (0.0, 1.0));
}

/// A member who has used nothing yet has still been answered: nothing spent is a figure.
#[test]
fn no_rows_is_nothing_spent() {
    let spent = parse(&breakdown(&[]), now()).unwrap();
    assert_eq!((spent.last_7_days, spent.last_30_days), (0.0, 0.0));
}

/// Anything counted in another unit (the app also reads token breakdowns) is not credits.
#[test]
fn only_a_breakdown_counted_in_credits_is_read() {
    let mut tokens = breakdown(&[("2026-09-24", &[5.0])]);
    tokens["units"] = json!("tokens");
    let mut unitless = breakdown(&[("2026-09-24", &[5.0])]);
    unitless.as_object_mut().unwrap().remove("units");

    assert_eq!(parse(&tokens, now()), None);
    assert_eq!(parse(&unitless, now()), None);
    assert_eq!(parse(&json!({"units": "credits"}), now()), None);
}

#[test]
fn an_amount_that_is_not_a_count_of_credits_adds_nothing() {
    let mut payload = breakdown(&[("2026-09-24", &[10.0])]);
    payload["data"][0]["models"]
        .as_array_mut()
        .unwrap()
        .extend([json!({"credits": -5.0}), json!({"credits": "7"}), json!({})]);
    payload["data"].as_array_mut().unwrap().extend([
        json!({"date": "not a date", "models": [{"credits": 3.0}]}),
        json!({}),
    ]);

    let spent = parse(&payload, now()).unwrap();
    assert_eq!((spent.last_7_days, spent.last_30_days), (10.0, 10.0));
}

/// Without a freshness time of its own the figure is dated when it was read, so a remembered one
/// still says how old it is.
#[test]
fn a_missing_or_unreadable_freshness_dates_the_figure_when_it_was_read() {
    let mut missing = breakdown(&[]);
    missing.as_object_mut().unwrap().remove("data_freshness_ts");
    let mut unreadable = breakdown(&[]);
    unreadable["data_freshness_ts"] = json!("yesterday");

    let read_at = Some("2026-09-24T12:00:00Z".to_string());
    assert_eq!(parse(&missing, now()).unwrap().updated_at, read_at);
    assert_eq!(parse(&unreadable, now()).unwrap().updated_at, read_at);
}

/// The app asks for whole UTC days ending today: `start = today - (days - 1)`.
#[test]
fn asks_for_the_30_utc_days_ending_today_by_day() {
    assert_eq!(
        query_url("https://example.test/breakdown", today()),
        "https://example.test/breakdown?start_date=2026-08-26&end_date=2026-09-24&group_by=day"
    );
}

/// Codex's own `PlanType::is_workspace_account` (openai/codex rust-v0.156.1,
/// `codex-rs/protocol/src/account.rs`): team-like, business-like, education-like and enterprise.
#[test]
fn workspace_plans_are_the_ones_codex_counts_as_workspace_accounts() {
    for plan in [
        "team",
        "self_serve_business_prolite",
        "self_serve_business_usage_based",
        "business",
        "ent26",
        "enterprise_cbp_automation",
        "enterprise_cbp_usage_based",
        "enterprise",
        "edu",
        "edu_plus",
        "edu_pro",
    ] {
        assert!(is_codex_workspace_plan(plan), "{plan}");
    }
    for plan in [
        "free", "go", "plus", "pro", "prolite", "guest", "", "unknown",
    ] {
        assert!(!is_codex_workspace_plan(plan), "{plan}");
    }
}

/// A key no other test uses, so the backoff one test records never skips another's read.
fn account(name: &str) -> String {
    format!("test-account:{name}:{:?}", std::thread::current().id())
}

/// The seam both callers go through: one GET with the login's own token and workspace, for the 30
/// UTC days up to the injected clock.
#[test]
fn reads_with_the_logins_token_for_its_workspace() {
    let (url, request) = crate::http::serve_once_capturing(
        "200 OK",
        &[],
        &breakdown(&[("2026-09-24", &[7.5])]).to_string(),
    );

    let spent = read(&AccessToken::new("fixture-access"), "team", &url, now());
    let head = request.join().unwrap().head;

    assert!(
        head.lines()
            .next()
            .unwrap_or_default()
            .contains("?start_date=2026-08-26&end_date=2026-09-24&group_by=day "),
        "{head}"
    );
    assert!(head.contains("Bearer fixture-access"), "{head}");
    assert!(
        head.to_lowercase().contains("chatgpt-account-id: team"),
        "{head}"
    );
    assert_eq!(spent.map(|spent| spent.last_7_days), Some(7.5));
}

#[test]
fn a_refused_or_failed_read_is_no_figure() {
    let (url, request) = crate::http::serve_once("403 Forbidden", "{}");
    assert_eq!(read(&AccessToken::new("t"), "team", &url, now()), None);
    request.join().unwrap();
    assert_eq!(
        read(
            &AccessToken::new("t"),
            "team",
            &crate::http::refused_url(),
            now()
        ),
        None
    );
}

/// A listener that never answers: a request would sit in its backlog, where `accept` finds it.
fn never_asked() -> (std::net::TcpListener, String) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/breakdown", listener.local_addr().unwrap());
    (listener, url)
}

fn was_asked(listener: &std::net::TcpListener) -> bool {
    listener.accept().is_ok()
}

/// A failing endpoint costs one request per backoff period, not one per refresh.
#[test]
fn a_failed_read_backs_off_before_the_account_is_asked_again() {
    let key = account("backoff");
    assert_eq!(
        read_backed_off(&projection(&key), &crate::http::refused_url(), now()),
        None
    );
    let (listener, url) = never_asked();

    assert_eq!(read_backed_off(&projection(&key), &url, now()), None);
    assert!(!was_asked(&listener), "asked again while backing off");

    // Another account is not held back by this one's failure.
    let (url, request) =
        crate::http::serve_once("200 OK", &breakdown(&[("2026-09-24", &[1.0])]).to_string());
    assert!(read_backed_off(&projection(&account("backoff-other")), &url, now()).is_some());
    request.join().unwrap();
}

#[test]
fn a_successful_read_does_not_hold_the_next_one_back() {
    let key = account("success");
    let body = breakdown(&[("2026-09-24", &[1.0])]).to_string();
    let (url, requests) =
        crate::http::serve_sequence(&[("200 OK", &[], &body), ("200 OK", &[], &body)]);

    assert!(read_backed_off(&projection(&key), &url, now()).is_some());
    assert!(read_backed_off(&projection(&key), &url, now()).is_some());
    assert_eq!(requests.join().unwrap().len(), 2);
}

/// The signed-in login's access projection, as the read's identity check hands it over.
fn access(key: &str) -> Option<CodexAccess> {
    Some(CodexAccess {
        observation_key: key.to_string(),
        workspace_id: "team".to_string(),
        token: AccessToken::new("native-access"),
    })
}

/// A saved account's projection, as the saved read builds it.
fn projection(key: &str) -> CodexAccess {
    CodexAccess {
        observation_key: key.to_string(),
        workspace_id: "team".to_string(),
        token: AccessToken::new("t"),
    }
}

/// The signed-in card has no token of its own (app-server reads it), so its login's access token is
/// used, for its own workspace, and the figure is that card's.
#[test]
fn the_signed_in_workspace_card_is_asked_with_its_logins_access_token() {
    let key = account("signed-in");
    let (url, request) = crate::http::serve_once_capturing(
        "200 OK",
        &[],
        &breakdown(&[("2026-09-24", &[18303.4])]).to_string(),
    );

    let spent = signed_in(
        access(&key).as_ref(),
        &Parsed::for_card(Some(&key), Some("self_serve_business_prolite")),
        &url,
        now(),
    );
    let head = request.join().unwrap().head;

    assert!(head.contains("Bearer native-access"), "{head}");
    assert!(
        head.to_lowercase().contains("chatgpt-account-id: team"),
        "{head}"
    );
    assert_eq!(spent.map(|spent| spent.last_7_days), Some(18303.4));
}

/// A personal plan pools nothing and is never asked, whatever access it is handed.
#[test]
fn a_personal_signed_in_card_is_never_asked() {
    let (listener, url) = never_asked();
    for plan in [Some("pro"), Some("plus"), None] {
        let key = account("personal");
        let spent = signed_in(
            access(&key).as_ref(),
            &Parsed::for_card(Some(&key), plan),
            &url,
            now(),
        );
        assert_eq!(spent, None, "{plan:?}");
    }
    assert!(!was_asked(&listener));
}

/// Another account's token is never spent on this card, and no access at all is no figure.
#[test]
fn access_that_is_not_the_cards_account_is_not_asked() {
    let (listener, url) = never_asked();
    for handed in [access(&account("other-login")), None] {
        let spent = signed_in(
            handed.as_ref(),
            &Parsed::for_card(Some("the-cards-account"), Some("business")),
            &url,
            now(),
        );
        assert_eq!(spent, None);
    }
    assert!(!was_asked(&listener));
}

fn failed_before(key: &str, count: u32) {
    MEMO.failed_before(key, count);
}

fn backoff_of(key: &str) -> Option<(Instant, u32)> {
    MEMO.backoff_of(key)
}

/// Another failure once the wait has run out counts toward a longer one.
#[test]
fn a_further_failure_waits_longer() {
    let key = account("escalates");
    failed_before(&key, 2);

    let before = Instant::now();
    assert_eq!(
        read_backed_off(&projection(&key), &crate::http::refused_url(), now()),
        None
    );
    let after = Instant::now();

    let (until, count) = backoff_of(&key).unwrap();
    let wait = backend_memo::backoff_delay(3, crate::limits_refresh::poll_interval());
    assert_eq!(count, 3);
    assert!(until >= before + wait && until <= after + wait);
}

/// A success after failures clears the account's backoff, so its next failure starts from one
/// interval again.
#[test]
fn a_success_after_failures_clears_the_accounts_backoff() {
    let key = account("recovers");
    failed_before(&key, 3);
    let (url, request) =
        crate::http::serve_once("200 OK", &breakdown(&[("2026-09-24", &[1.0])]).to_string());

    assert!(read_backed_off(&projection(&key), &url, now()).is_some());
    request.join().unwrap();

    assert_eq!(backoff_of(&key), None);
}
