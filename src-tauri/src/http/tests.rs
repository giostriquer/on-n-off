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

/// The Limits services answer a 429 when their rate limit is exhausted, saying when to retry.
#[test]
fn a_throttled_get_is_rate_limited_until_its_retry_after() {
    for (headers, reset) in [
        (&["Retry-After: 30"][..], RateLimitReset::RetryAfter(30)),
        (&[][..], RateLimitReset::Unknown),
    ] {
        let (url, request) = serve_once_capturing("429 Too Many Requests", headers, "{}");
        assert_eq!(
            get_json(&url, &[]),
            Err(HttpError::RateLimited(reset)),
            "{headers:?}"
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
/// leave `request.join()` waiting until CI's job timeout. Every loopback server here accepts
/// through `accept_within`; the deadline is shortened so the test stays quick, where the servers
/// wait `ACCEPT_DEADLINE`.
#[test]
fn a_loopback_server_nobody_calls_gives_up_at_its_deadline() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let server = std::thread::spawn(move || {
        accept_within(&listener, Duration::from_millis(50));
    });
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
        .expect_err("a server nobody called has no connection to hand over");
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

/// The server polls for its connection without blocking, and on macOS and Windows the socket it
/// accepts starts out non-blocking too. A client that has connected but not yet written must
/// still be read, not fail the server with `WouldBlock`: the code under test is often slower than
/// the accept poll, above all on a loaded CI runner.
#[test]
fn a_one_shot_server_reads_a_request_that_arrives_after_it_accepts() {
    use std::io::{Read, Write};

    let (url, server) = serve_once("200 OK", "{}");
    let address = url
        .strip_prefix("http://")
        .and_then(|rest| rest.split('/').next())
        .unwrap();
    let mut client = std::net::TcpStream::connect(address).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    client
        .write_all(b"GET /usage HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let mut reply = String::new();
    client.read_to_string(&mut reply).unwrap();

    assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
    let head = server.join().expect("the server read the late request");
    assert!(head.starts_with("GET /usage "), "{head}");
}

#[test]
fn a_refused_connection_is_a_network_error() {
    assert!(matches!(
        get_json(&refused_url(), &[]),
        Err(HttpError::Network(_))
    ));
}

/// A refused URL has to stay refused while other tests open servers, and has to fail at once on
/// every OS. A port given back by a dropped listener is neither: the OS may hand it to the next
/// test's server, and Windows retries a connect it answers with a reset for about 2 s.
#[test]
fn a_refused_url_fails_at_once_and_no_server_can_take_it() {
    let url = refused_url();
    let address = url
        .strip_prefix("http://")
        .and_then(|rest| rest.split('/').next())
        .unwrap();
    let squatter = std::net::TcpListener::bind(address).unwrap();
    assert_ne!(
        squatter.local_addr().unwrap().to_string(),
        address,
        "a server can listen on the refused address"
    );

    let started = std::time::Instant::now();
    let result = get_json(&url, &[]);
    assert!(matches!(result, Err(HttpError::Network(_))), "{result:?}");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "refused after {:?}",
        started.elapsed()
    );
}

/// `ureq` reads `HTTP(S)_PROXY` and `ALL_PROXY` when the builder is made, and would send every
/// loopback test server's request to that proxy. A builder given a proxy outright stands in for
/// such an environment without touching the process-wide variables other tests read.
#[test]
fn test_requests_ignore_a_proxy_the_environment_names() {
    let proxied = || {
        ureq::Agent::config_builder().proxy(Some(ureq::Proxy::new("http://127.0.0.1:9").unwrap()))
    };
    assert!(agent_config(proxied(), false).proxy().is_none());
    assert!(
        agent_config(proxied(), true).proxy().is_some(),
        "the app keeps the proxy its environment names"
    );
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

    // A token issuer's 429 is no rate limit the renewal reads as one, whatever it says.
    let (url, request) = serve_once_capturing("429 Too Many Requests", &["Retry-After: 30"], "{}");
    assert_eq!(
        post_grant(&url, &serde_json::json!({})),
        Err(HttpError::Status(429))
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
