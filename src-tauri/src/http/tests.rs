use super::*;

#[test]
fn returns_parsed_json_and_sends_the_given_headers() {
    let (url, request) = serve_once("200 OK", r#"{"ok":true}"#);
    let value = get_json(&url, &[("Authorization", "Bearer t0k"), ("X-Probe", "1")]).unwrap();
    assert_eq!(value["ok"], serde_json::Value::Bool(true));
    let head = request.join().unwrap();
    assert_eq!(
        head_header(&head, "authorization"),
        Some("Bearer t0k"),
        "{head}"
    );
    assert_eq!(head_header(&head, "x-probe"), Some("1"), "{head}");
    assert!(
        head_header(&head, "user-agent").is_some_and(|agent| agent.starts_with("on-n-off/")),
        "{head}"
    );
    assert_eq!(
        head_header(&head, "accept"),
        Some("application/json"),
        "{head}"
    );
}

#[test]
fn unauthorized_and_forbidden_are_unauthorized() {
    for status in ["401 Unauthorized", "403 Forbidden"] {
        let (url, request) = serve_once(status, r#"{"error":"nope"}"#);
        assert_eq!(
            get_json(&url, &[]),
            Err(HttpError::Unauthorized),
            "{status}"
        );
        request.join().unwrap();
    }
}

#[test]
fn other_error_statuses_keep_their_code() {
    let (url, request) = serve_once("503 Service Unavailable", "");
    assert_eq!(get_json(&url, &[]), Err(HttpError::Status(503)));
    request.join().unwrap();
}

#[test]
fn a_non_json_body_is_a_parse_error() {
    let (url, request) = serve_once("200 OK", "<html>");
    assert!(matches!(get_json(&url, &[]), Err(HttpError::Parse(_))));
    request.join().unwrap();
}

/// A regression that stops the request from being made has to fail its test with a message, not
/// leave `request.join()` waiting until CI's job timeout. The deadline is shortened here so the
/// test stays quick; `serve_once` waits `ACCEPT_DEADLINE`.
#[test]
fn a_one_shot_server_nobody_calls_gives_up_at_its_deadline() {
    let (_url, server) = serve_once_within("200 OK", "{}", Duration::from_millis(50));
    let started = std::time::Instant::now();
    while !server.is_finished() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the server is still waiting for a request long after its deadline"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let panic = server
        .join()
        .expect_err("a server nobody called has no request to report");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or_default();
    assert!(
        message.contains("no request reached the loopback server"),
        "{message}"
    );
}

#[test]
fn a_refused_connection_is_a_network_error() {
    assert!(matches!(
        get_json(&refused_url(), &[]),
        Err(HttpError::Network(_))
    ));
}

#[test]
fn post_json_sends_a_bearer_json_body_and_parses_the_reply() {
    let (url, request) = serve_once_capturing("200 OK", &[], r#"{"data":{"ok":true}}"#);
    let body = serde_json::json!({ "query": "{ viewer { login } }", "variables": { "n": 1 } });
    let value = post_json(&url, "t0k", &body).unwrap();
    assert_eq!(value["data"]["ok"], Value::Bool(true));
    let captured = request.join().unwrap();
    assert!(
        captured.head.starts_with("POST /graphql "),
        "{}",
        captured.head
    );
    assert_eq!(
        head_header(&captured.head, "authorization"),
        Some("Bearer t0k"),
        "{}",
        captured.head
    );
    assert_eq!(
        head_header(&captured.head, "content-type"),
        Some("application/json"),
        "{}",
        captured.head
    );
    assert_eq!(
        head_header(&captured.head, "accept"),
        Some("application/json"),
        "{}",
        captured.head
    );
    assert!(
        head_header(&captured.head, "user-agent")
            .is_some_and(|agent| agent.starts_with("on-n-off/")),
        "{}",
        captured.head
    );
    assert_eq!(
        serde_json::from_str::<Value>(&captured.body).unwrap(),
        body,
        "{}",
        captured.body
    );
}

#[test]
fn post_json_maps_401_to_unauthorized_but_a_plain_403_keeps_its_code() {
    let (url, request) =
        serve_once_capturing("401 Unauthorized", &[], r#"{"message":"Bad credentials"}"#);
    assert_eq!(
        post_json(&url, "t", &serde_json::json!({})),
        Err(HttpError::Unauthorized)
    );
    request.join().unwrap();

    let (url, request) = serve_once_capturing(
        "403 Forbidden",
        &[],
        r#"{"message":"Resource not accessible by integration"}"#,
    );
    assert_eq!(
        post_json(&url, "t", &serde_json::json!({})),
        Err(HttpError::Status(403))
    );
    request.join().unwrap();
}

#[test]
fn post_json_reports_an_exhausted_primary_rate_limit_with_its_reset_instant() {
    let (url, request) = serve_once_capturing(
        "403 Forbidden",
        &["X-RateLimit-Remaining: 0", "X-RateLimit-Reset: 1787022473"],
        r#"{"message":"API rate limit exceeded"}"#,
    );
    assert_eq!(
        post_json(&url, "t", &serde_json::json!({})),
        Err(HttpError::RateLimited(RateLimitReset::At(1_787_022_473)))
    );
    request.join().unwrap();
}

#[test]
fn post_json_passes_a_secondary_limit_retry_after_through_as_seconds() {
    let (url, request) = serve_once_capturing(
        "429 Too Many Requests",
        &["Retry-After: 60"],
        r#"{"message":"You have exceeded a secondary rate limit"}"#,
    );
    assert_eq!(
        post_json(&url, "t", &serde_json::json!({})),
        Err(HttpError::RateLimited(RateLimitReset::RetryAfter(60)))
    );
    request.join().unwrap();
}

#[test]
fn post_json_without_a_reset_hint_is_still_rate_limited() {
    let (url, request) =
        serve_once_capturing("429 Too Many Requests", &[], r#"{"message":"slow down"}"#);
    assert_eq!(
        post_json(&url, "t", &serde_json::json!({})),
        Err(HttpError::RateLimited(RateLimitReset::Unknown))
    );
    request.join().unwrap();
}

#[test]
fn post_json_other_statuses_parse_failures_and_refusals_map_like_get() {
    let (url, request) = serve_once_capturing("500 Internal Server Error", &[], "");
    assert_eq!(
        post_json(&url, "t", &serde_json::json!({})),
        Err(HttpError::Status(500))
    );
    request.join().unwrap();

    let (url, request) = serve_once_capturing("200 OK", &[], "<html>");
    assert!(matches!(
        post_json(&url, "t", &serde_json::json!({})),
        Err(HttpError::Parse(_))
    ));
    request.join().unwrap();

    assert!(matches!(
        post_json(&refused_url(), "t", &serde_json::json!({})),
        Err(HttpError::Network(_))
    ));
}

#[test]
fn rate_limited_errors_describe_themselves_without_leaking_anything() {
    assert_eq!(
        HttpError::RateLimited(RateLimitReset::At(1)).to_string(),
        "rate limited"
    );
}

#[test]
fn post_grant_sends_the_body_without_an_authorization_header() {
    let (url, request) = serve_once_capturing("200 OK", &[], r#"{"access_token":"new"}"#);
    let body = serde_json::json!({ "grant_type": "refresh_token", "refresh_token": "rt" });
    let value = post_grant(&url, &body).unwrap();
    assert_eq!(value["access_token"], "new");

    let captured = request.join().unwrap();
    assert!(
        !captured.head.to_lowercase().contains("authorization:"),
        "a grant carries its own credential in the body: {}",
        captured.head
    );
    assert_eq!(
        head_header(&captured.head, "content-type"),
        Some("application/json"),
        "{}",
        captured.head
    );
    assert_eq!(
        serde_json::from_str::<Value>(&captured.body).unwrap(),
        body,
        "the grant arrives as sent"
    );
}

/// The distinctions the Claude renewal decides on: 400 means the grant itself was refused and the
/// user has to sign in again, anything else means try later.
#[test]
fn post_grant_keeps_a_refused_grant_apart_from_a_transport_failure() {
    let (url, request) =
        serve_once_capturing("400 Bad Request", &[], r#"{"error":"invalid_grant"}"#);
    assert_eq!(
        post_grant(&url, &serde_json::json!({})),
        Err(HttpError::Status(400))
    );
    request.join().unwrap();

    let (url, request) = serve_once_capturing("401 Unauthorized", &[], "{}");
    assert_eq!(
        post_grant(&url, &serde_json::json!({})),
        Err(HttpError::Unauthorized)
    );
    request.join().unwrap();

    let (url, request) = serve_once_capturing("503 Service Unavailable", &[], "{}");
    assert_eq!(
        post_grant(&url, &serde_json::json!({})),
        Err(HttpError::Status(503))
    );
    request.join().unwrap();

    assert!(matches!(
        post_grant(&refused_url(), &serde_json::json!({})),
        Err(HttpError::Network(_))
    ));
}

/// A 2xx whose body is not JSON. The renewal reads that as the token having been rotated behind a
/// reply it could not parse, so the taxonomy has to keep it out of the retryable bucket.
#[test]
fn post_grant_reports_an_unreadable_success_as_a_parse_failure() {
    let (url, request) = serve_once_capturing("200 OK", &[], "<html>gateway</html>");
    assert!(matches!(
        post_grant(&url, &serde_json::json!({})),
        Err(HttpError::Parse(_))
    ));
    request.join().unwrap();
}
