use super::*;

#[test]
fn returns_parsed_json_and_sends_the_given_headers() {
    let (url, request) = serve_once("200 OK", r#"{"ok":true}"#);
    let value = get_json(&url, &[("Authorization", "Bearer t0k"), ("X-Probe", "1")]).unwrap();
    assert_eq!(value["ok"], serde_json::Value::Bool(true));
    let head = request.join().unwrap();
    assert!(head.contains("Authorization: Bearer t0k"), "{head}");
    assert!(head.contains("X-Probe: 1"), "{head}");
    assert!(head.contains("User-Agent: on-n-off/"), "{head}");
    assert!(head.contains("Accept: application/json"), "{head}");
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
    assert!(
        captured.head.contains("Authorization: Bearer t0k"),
        "{}",
        captured.head
    );
    assert!(
        captured.head.contains("Content-Type: application/json"),
        "{}",
        captured.head
    );
    assert!(
        captured.head.contains("Accept: application/json"),
        "{}",
        captured.head
    );
    assert!(
        captured.head.contains("User-Agent: on-n-off/"),
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
