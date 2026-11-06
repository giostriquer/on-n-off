use super::*;
use crate::cli_stub::CliStub;
use crate::github::fixtures::{authorization, gh, gh_runs};
use crate::http::{head_header, refused_url, serve_sequence};
use crate::paths::scratch_dir;
use serde_json::json;

const NOW: i64 = 1_787_000_000;
const SEARCH: &str = r#"{"data":{"viewer":{"login":"octocat"},"rateLimit":{"remaining":4998,"resetAt":"2026-08-24T23:00:00Z"}}}"#;

fn github<'a>(
    memo: &'a HttpMemo,
    cli: &'a AgentCli,
    url: &'a str,
    now_secs: i64,
) -> HttpGithub<'a> {
    HttpGithub {
        memo,
        cli,
        graphql_url: url,
        events_url: url,
        now_secs,
    }
}

fn budget(remaining: u64, reset_at: &str) -> String {
    json!({ "data": { "rateLimit": { "remaining": remaining, "resetAt": reset_at } } }).to_string()
}

fn at(epoch_secs: i64) -> String {
    chrono::DateTime::from_timestamp(epoch_secs, 0)
        .unwrap()
        .to_rfc3339()
}

fn ask(
    memo: &HttpMemo,
    cli: &AgentCli,
    now_secs: i64,
    reply: (&str, &[&str], &str),
) -> Result<Value, Failure> {
    let (url, server) = serve_sequence(&[reply]);
    let answer = github(memo, cli, &url, now_secs).graphql(&json!({ "query": "{}" }));
    assert_eq!(server.join().unwrap().len(), 1);
    answer
}

fn hint_of(failure: Option<Failure>) -> String {
    failure.map(|failure| failure.hint).unwrap_or_default()
}

#[test]
fn every_request_carries_the_gh_token_read_once_per_app_run() {
    let memo = HttpMemo::new();
    let dir = scratch_dir("gh-connection-token");
    let cli = gh(&dir, "gho_t");
    let (url, server) = serve_sequence(&[
        ("200 OK", &[], SEARCH),
        ("200 OK", &[], "{}"),
        ("200 OK", &[], "[]"),
    ]);
    let connection = github(&memo, &cli, &url, NOW);

    assert_eq!(
        connection.graphql(&json!({ "query": "a" })).unwrap()["data"]["viewer"]["login"],
        "octocat"
    );
    connection.graphql(&json!({ "query": "b" })).unwrap();
    assert_eq!(connection.events("octocat", 2).unwrap(), json!([]));

    let requests = server.join().unwrap();
    assert!(requests
        .iter()
        .all(|request| authorization(request) == "Bearer gho_t"));
    assert!(
        requests[0].head.starts_with("POST /graphql "),
        "{}",
        requests[0].head
    );
    assert_eq!(
        serde_json::from_str::<Value>(&requests[1].body).unwrap(),
        json!({ "query": "b" })
    );
    assert!(
        requests[2]
            .head
            .starts_with("GET /graphql/octocat/events?per_page=100&page=2 "),
        "{}",
        requests[2].head
    );
    assert_eq!(
        head_header(&requests[2].head, "accept"),
        Some("application/vnd.github+json")
    );
    assert_eq!(gh_runs(&dir), 1, "`gh auth token` ran once per app run");
}

#[test]
fn a_missing_gh_says_how_to_install_it_for_every_request() {
    let memo = HttpMemo::new();
    let dir = scratch_dir("gh-connection-missing");
    let cli = AgentCli::new(dir.join("gh").to_string_lossy().as_ref());
    let refused = refused_url();
    let connection = github(&memo, &cli, &refused, NOW);

    for failure in [
        connection.graphql(&json!({})).unwrap_err(),
        connection.events("octocat", 1).unwrap_err(),
    ] {
        assert_eq!(failure.status, GithubStatus::GhMissing);
        assert!(failure.hint.contains("gh auth login"), "{failure:?}");
    }
}

#[test]
fn a_signed_out_gh_says_so_without_leaking_its_stderr() {
    let memo = HttpMemo::new();
    let cli = CliStub::new("gh")
        .stderr("no oauth token found: secret-path")
        .exit(1)
        .cli(&scratch_dir("gh-connection-signed-out"));

    let failure = github(&memo, &cli, &refused_url(), NOW)
        .graphql(&json!({}))
        .unwrap_err();

    assert_eq!(failure.status, GithubStatus::GhNotLoggedIn);
    assert!(failure.hint.contains("gh auth login"), "{failure:?}");
    assert!(
        !format!("{failure:?}").contains("secret-path"),
        "{failure:?}"
    );
}

#[test]
fn a_rejected_token_is_re_read_from_gh_once() {
    let memo = HttpMemo::new();
    let home = scratch_dir("gh-connection-401-retry");
    let stale = gh(&home.join("stale"), "gho_stale");
    ask(&memo, &stale, NOW, ("200 OK", &[], SEARCH)).unwrap();

    let fresh_dir = home.join("fresh");
    let fresh = gh(&fresh_dir, "gho_fresh");
    let (url, server) = serve_sequence(&[
        ("401 Unauthorized", &[], r#"{"message":"Bad credentials"}"#),
        ("200 OK", &[], SEARCH),
    ]);
    let reply = github(&memo, &fresh, &url, NOW + 1).graphql(&json!({}));

    assert!(reply.is_ok(), "{reply:?}");
    let requests = server.join().unwrap();
    assert_eq!(authorization(&requests[0]), "Bearer gho_stale");
    assert_eq!(authorization(&requests[1]), "Bearer gho_fresh");
    assert_eq!(gh_runs(&fresh_dir), 1);
}

#[test]
fn a_token_rejected_twice_is_reported_and_forgotten() {
    let memo = HttpMemo::new();
    let dir = scratch_dir("gh-connection-401-twice");
    let cli = gh(&dir, "gho_x");
    let (url, server) = serve_sequence(&[
        ("401 Unauthorized", &[], "{}"),
        ("401 Unauthorized", &[], "{}"),
    ]);

    let failure = github(&memo, &cli, &url, NOW)
        .graphql(&json!({}))
        .unwrap_err();

    assert_eq!(failure.status, GithubStatus::TokenRejected);
    assert!(failure.hint.contains("gh auth login"), "{failure:?}");
    assert!(!failure.hint.contains("gho_x"), "{failure:?}");
    assert_eq!(server.join().unwrap().len(), 2);
    assert_eq!(gh_runs(&dir), 2);
    assert_eq!(
        memo.token.lookup(|| Err(TokenError::NotLoggedIn)),
        Err(TokenError::NotLoggedIn),
        "the rejected token is not kept"
    );
}

#[test]
fn an_exhausted_rate_limit_pauses_graphql_until_it_resets() {
    let memo = HttpMemo::new();
    let cli = gh(&scratch_dir("gh-connection-rate-limited"), "gho_t");
    let reset = format!("X-RateLimit-Reset: {}", NOW + 300);

    let failure = ask(
        &memo,
        &cli,
        NOW,
        (
            "403 Forbidden",
            &["X-RateLimit-Remaining: 0", &reset],
            r#"{"message":"API rate limit exceeded"}"#,
        ),
    )
    .unwrap_err();
    assert_eq!(failure.status, GithubStatus::RateLimited);
    assert_eq!(
        failure.hint,
        "GitHub rate limit reached — polling resumes in 5 min."
    );

    let refused = refused_url();
    let paused = github(&memo, &cli, &refused, NOW + 10);
    assert_eq!(
        hint_of(paused.reads_paused()),
        "GitHub rate limit reached — polling resumes in 5 min."
    );
    assert_eq!(
        paused.graphql(&json!({})).unwrap_err().status,
        GithubStatus::RateLimited,
        "no request while paused"
    );
    assert!(
        github(&memo, &cli, &refused_url(), NOW + 299)
            .reads_paused()
            .is_some(),
        "still paused one second before the reset"
    );
    assert_eq!(
        github(&memo, &cli, &refused_url(), NOW + 300).reads_paused(),
        None
    );
    assert!(ask(&memo, &cli, NOW + 300, ("200 OK", &[], SEARCH)).is_ok());
}

#[test]
fn rate_limit_replies_without_a_usable_reset_pause_for_one_minute() {
    let past_reset = format!("X-RateLimit-Reset: {}", NOW - 10);
    let cases: [(&str, &[&str]); 2] = [
        ("429 Too Many Requests", &[]),
        ("403 Forbidden", &["X-RateLimit-Remaining: 0", &past_reset]),
    ];
    for (index, (status_line, headers)) in cases.into_iter().enumerate() {
        let memo = HttpMemo::new();
        let cli = gh(
            &scratch_dir(&format!("gh-connection-fallback-{index}")),
            "gho_t",
        );

        let failure = ask(&memo, &cli, NOW, (status_line, headers, "{}")).unwrap_err();
        assert_eq!(failure.status, GithubStatus::RateLimited, "{status_line}");
        assert!(failure.hint.contains("1 min"), "{failure:?}");
        assert!(
            github(&memo, &cli, &refused_url(), NOW + 30)
                .reads_paused()
                .is_some(),
            "{status_line}: paused for a minute"
        );
        assert!(
            ask(&memo, &cli, NOW + 60, ("200 OK", &[], SEARCH)).is_ok(),
            "{status_line}: resumes after a minute"
        );
    }
}

#[test]
fn a_reply_reporting_a_nearly_spent_budget_pauses_graphql_until_its_reset() {
    let memo = HttpMemo::new();
    let cli = gh(&scratch_dir("gh-connection-low-budget"), "gho_t");

    let reply = ask(
        &memo,
        &cli,
        NOW,
        ("200 OK", &[], &budget(10, &at(NOW + 120))),
    )
    .unwrap();
    assert_eq!(reply["data"]["rateLimit"]["remaining"], 10);

    let refused = refused_url();
    let paused = github(&memo, &cli, &refused, NOW + 5);
    assert_eq!(
        paused.reads_paused(),
        Some(Failure::new(
            GithubStatus::RateLimited,
            "GitHub rate limit reached — polling resumes in 2 min."
        ))
    );
    assert_eq!(
        paused.graphql(&json!({})).unwrap_err().status,
        GithubStatus::RateLimited
    );
}

#[test]
fn the_low_budget_pause_starts_below_fifty_points() {
    let fine = HttpMemo::new();
    let cli = gh(&scratch_dir("gh-connection-budget"), "gho_t");
    ask(
        &fine,
        &cli,
        NOW,
        ("200 OK", &[], &budget(50, &at(NOW + 600))),
    )
    .unwrap();
    assert_eq!(
        github(&fine, &cli, &refused_url(), NOW + 56).reads_paused(),
        None,
        "fifty points left is not a pause"
    );

    let low = HttpMemo::new();
    ask(
        &low,
        &cli,
        NOW,
        ("200 OK", &[], &budget(49, &at(NOW + 600))),
    )
    .unwrap();
    assert_eq!(
        hint_of(github(&low, &cli, &refused_url(), NOW + 56).reads_paused()),
        "GitHub rate limit reached — polling resumes in 10 min."
    );
}

#[test]
fn an_unreadable_reset_instant_with_a_low_budget_pauses_for_one_minute() {
    let memo = HttpMemo::new();
    let cli = gh(&scratch_dir("gh-connection-budget-unreadable"), "gho_t");
    ask(&memo, &cli, NOW, ("200 OK", &[], &budget(3, "soon"))).unwrap();

    assert!(github(&memo, &cli, &refused_url(), NOW + 30)
        .reads_paused()
        .is_some());
    assert_eq!(
        github(&memo, &cli, &refused_url(), NOW + 60).reads_paused(),
        None
    );
}

#[test]
fn a_low_budget_whose_reset_the_clock_has_passed_pauses_nothing() {
    let memo = HttpMemo::new();
    let cli = gh(&scratch_dir("gh-connection-budget-renewed"), "gho_t");
    for reset in [NOW - 30, NOW] {
        ask(&memo, &cli, NOW, ("200 OK", &[], &budget(3, &at(reset)))).unwrap();
        assert_eq!(
            github(&memo, &cli, &refused_url(), NOW).reads_paused(),
            None,
            "reset at {reset}"
        );
    }
    let failure = ask(
        &memo,
        &cli,
        NOW,
        (
            "429 Too Many Requests",
            &[
                "X-RateLimit-Remaining: 0",
                &format!("X-RateLimit-Reset: {}", NOW - 30),
            ],
            "{}",
        ),
    )
    .unwrap_err();
    assert_eq!(
        failure.hint, "GitHub rate limit reached — polling resumes in 1 min.",
        "a refused request whose reset has passed still waits a minute"
    );
    assert!(
        github(&memo, &cli, &refused_url(), NOW + 30)
            .reads_paused()
            .is_some(),
        "a refused request whose reset has passed pauses reads for that minute"
    );
}

#[test]
fn unreachable_and_unreadable_replies_report_network() {
    let memo = HttpMemo::new();
    let cli = gh(&scratch_dir("gh-connection-network"), "gho_t");

    let refused = github(&memo, &cli, &refused_url(), NOW)
        .graphql(&json!({}))
        .unwrap_err();
    assert_eq!(refused.status, GithubStatus::Network);
    assert!(
        refused.hint.starts_with("Could not reach GitHub"),
        "{refused:?}"
    );

    let unreadable = ask(&memo, &cli, NOW, ("200 OK", &[], "<html>")).unwrap_err();
    assert_eq!(unreadable.status, GithubStatus::Network);
    assert!(
        unreadable
            .hint
            .starts_with("Could not reach GitHub (unreadable response"),
        "{unreadable:?}"
    );
    assert_eq!(
        github(&memo, &cli, &refused_url(), NOW).reads_paused(),
        None
    );
}

#[test]
fn a_rate_limited_events_read_pauses_only_the_events_until_its_reset() {
    let reset = format!("X-RateLimit-Reset: {}", NOW + 300);
    let cases: [(&str, &[&str], i64); 3] = [
        ("403 Forbidden", &["X-RateLimit-Remaining: 0", &reset], 300),
        ("429 Too Many Requests", &["Retry-After: 120"], 120),
        ("429 Too Many Requests", &[], 60),
    ];
    for (index, (status_line, headers, wait)) in cases.into_iter().enumerate() {
        let memo = HttpMemo::new();
        let cli = gh(
            &scratch_dir(&format!("gh-connection-events-limited-{index}")),
            "gho_t",
        );
        let (url, server) = serve_sequence(&[(status_line, headers, "{}")]);

        let failure = github(&memo, &cli, &url, NOW).events("octocat", 1);
        assert_eq!(server.join().unwrap().len(), 1, "{status_line}");
        assert_eq!(
            failure,
            Err(Failure::new(
                GithubStatus::RateLimited,
                format!(
                    "GitHub rate limit reached — your events are read again in {} min.",
                    wait / 60
                )
            )),
            "{status_line}"
        );

        let refused = refused_url();
        let paused = github(&memo, &cli, &refused, NOW + wait - 1);
        assert_eq!(
            paused.events("octocat", 1).unwrap_err().status,
            GithubStatus::RateLimited,
            "{status_line}: no events request while paused"
        );
        assert_eq!(
            paused.reads_paused(),
            None,
            "{status_line}: the search keeps its own budget"
        );
        assert!(ask(&memo, &cli, NOW + wait - 1, ("200 OK", &[], SEARCH)).is_ok());

        let (url, server) = serve_sequence(&[("200 OK", &[], "[]")]);
        assert_eq!(
            github(&memo, &cli, &url, NOW + wait).events("octocat", 1),
            Ok(json!([])),
            "{status_line}: read again at the reset"
        );
        server.join().unwrap();
    }
}

#[test]
fn a_forbidden_events_read_github_does_not_call_a_rate_limit_keeps_its_status() {
    let memo = HttpMemo::new();
    let cli = gh(&scratch_dir("gh-connection-events-forbidden"), "gho_t");
    let (url, server) = serve_sequence(&[
        ("403 Forbidden", &[], r#"{"message":"Forbidden"}"#),
        ("200 OK", &[], "[]"),
    ]);
    let connection = github(&memo, &cli, &url, NOW);

    assert_eq!(
        connection.events("octocat", 1),
        Err(Failure::new(
            GithubStatus::Network,
            "Could not reach GitHub (HTTP 403)."
        ))
    );
    assert_eq!(connection.events("octocat", 1), Ok(json!([])));
    assert_eq!(server.join().unwrap().len(), 2);
}

#[test]
fn an_events_read_re_reads_a_rejected_token_once_and_forgets_one_rejected_twice() {
    let memo = HttpMemo::new();
    let home = scratch_dir("gh-connection-events-401");
    let stale = gh(&home.join("stale"), "gho_stale");
    ask(&memo, &stale, NOW, ("200 OK", &[], SEARCH)).unwrap();

    let fresh_dir = home.join("fresh");
    let fresh = gh(&fresh_dir, "gho_fresh");
    let (url, server) = serve_sequence(&[
        ("401 Unauthorized", &[], r#"{"message":"Bad credentials"}"#),
        ("200 OK", &[], "[]"),
    ]);
    let page = github(&memo, &fresh, &url, NOW + 1).events("octocat", 1);
    assert_eq!(page, Ok(json!([])));
    let requests = server.join().unwrap();
    assert_eq!(authorization(&requests[0]), "Bearer gho_stale");
    assert_eq!(authorization(&requests[1]), "Bearer gho_fresh");
    assert_eq!(gh_runs(&fresh_dir), 1);

    let (url, server) = serve_sequence(&[
        ("401 Unauthorized", &[], "{}"),
        ("401 Unauthorized", &[], "{}"),
    ]);
    assert_eq!(
        github(&memo, &fresh, &url, NOW + 2).events("octocat", 1),
        Err(Failure::new(
            GithubStatus::TokenRejected,
            "GitHub rejected the `gh` login — run `gh auth login` again, then refresh."
        ))
    );
    assert_eq!(server.join().unwrap().len(), 2);
    assert_eq!(gh_runs(&fresh_dir), 2);
    assert_eq!(
        memo.token.lookup(|| Err(TokenError::NotLoggedIn)),
        Err(TokenError::NotLoggedIn),
        "the rejected token is not kept"
    );
}
