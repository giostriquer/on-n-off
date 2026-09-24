use super::*;
use crate::dto::LimitsResetCreditsDto;
use crate::http::{serve_once, serve_once_capturing};
use serde_json::json;

fn identity(provider: AgentId) -> Identity {
    Identity {
        provider,
        user_id: "user".into(),
        workspace_id: "team".into(),
    }
}

#[test]
fn saved_claude_reads_verified_usage_without_a_native_login() {
    let (profile, p) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"user","email":"you@example.com"},"organization":{"uuid":"team"}}"#,
    );
    let (usage, u) = serve_once_capturing("200 OK", &[], r#"{"seven_day":{"utilization":61}}"#);
    let auth = json!({"claudeAiOauth":{"accessToken":"fixture-access"}});
    let dto = read_at(
        &identity(AgentId::Claude),
        &auth,
        &profile,
        &usage,
        "unused",
        "unused",
    )
    .unwrap();
    p.join().unwrap();
    let request = u.join().unwrap();
    assert!(request.head.contains("Bearer fixture-access"));
    assert_eq!(dto.windows[0].used_percent, 61.0);
    assert!(!dto.windows[0].observed_at.is_empty());
    assert!(!dto.current_account);
    assert_eq!(
        dto.account.unwrap().id,
        identity(AgentId::Claude).observation_key()
    );
}

#[test]
fn saved_claude_reads_the_accounts_saved_resets_from_the_same_request() {
    let (profile, p) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"user","email":"you@example.com"},"organization":{"uuid":"team"}}"#,
    );
    let (usage, u) = serve_once_capturing(
        "200 OK",
        &[],
        r#"{"seven_day":{"utilization":61},"cedar_ember":{"eligible":true,"grants":[{"id":"launch","resets_left":1,"ends_at":"2026-10-05T00:00:00Z"}]}}"#,
    );
    let auth = json!({"claudeAiOauth":{"accessToken":"fixture-access"}});
    let dto = read_at(
        &identity(AgentId::Claude),
        &auth,
        &profile,
        &usage,
        "unused",
        "unused",
    )
    .unwrap();
    p.join().unwrap();
    let request = u.join().unwrap();
    let request_line = request.head.lines().next().unwrap_or_default();
    assert!(
        request_line.contains("?cedar_ember=1&skip_spend=1 "),
        "{request_line}"
    );
    assert_eq!(dto.windows[0].used_percent, 61.0);
    assert_eq!(
        dto.reset_credits.map(|resets| resets.available_count),
        Some(1)
    );
}

/// On this path a rejected login renews the saved profile and spends its refresh token, so a
/// refusal of the optional reset query must never be read as one.
#[test]
fn saved_claude_falls_back_to_the_plain_read_when_the_reset_query_is_refused() {
    let (profile, p) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"user","email":"you@example.com"},"organization":{"uuid":"team"}}"#,
    );
    let (usage, u) = crate::http::serve_sequence(&[
        ("403 Forbidden", &[], "{}"),
        ("200 OK", &[], r#"{"seven_day":{"utilization":61}}"#),
    ]);
    let dto = read_at(
        &identity(AgentId::Claude),
        &json!({"claudeAiOauth":{"accessToken":"fixture-access"}}),
        &profile,
        &usage,
        "unused",
        "unused",
    )
    .unwrap();
    p.join().unwrap();
    let requests = u.join().unwrap();
    assert!(!requests[1]
        .head
        .lines()
        .next()
        .unwrap_or_default()
        .contains('?'));
    assert_eq!(dto.windows[0].used_percent, 61.0);
    assert_eq!(dto.reset_credits, None);
}

#[test]
fn saved_claude_keeps_weekly_primary_for_both_usage_formats() {
    use crate::dto::LimitWindowKind::{Model, Session, Weekly};

    for session_percent in [0, 67] {
        let payloads = [
            json!({
                "five_hour": {"utilization": session_percent},
                "seven_day": {"utilization": 99},
                "seven_day_opus": {"utilization": 100}
            }),
            json!({"limits": [
                {"kind": "session", "group": "session", "percent": session_percent},
                {"kind": "weekly_opus", "group": "weekly", "percent": 100},
                {"kind": "weekly_all", "group": "weekly", "percent": 99}
            ]}),
        ];
        for payload in payloads {
            let (profile, p) = serve_once(
                "200 OK",
                r#"{"account":{"uuid":"user"},"organization":{"uuid":"team"}}"#,
            );
            let (usage, u) = serve_once("200 OK", &payload.to_string());
            let dto = read_at(
                &identity(AgentId::Claude),
                &json!({"claudeAiOauth":{"accessToken":"fixture-access"}}),
                &profile,
                &usage,
                "unused",
                "unused",
            )
            .unwrap();
            p.join().unwrap();
            u.join().unwrap();

            assert!(!dto.current_account);
            assert_eq!(
                dto.windows
                    .iter()
                    .map(|window| (window.kind, window.used_percent))
                    .collect::<Vec<_>>(),
                vec![(Weekly, 99.0), (Session, f64::from(session_percent)), (Model, 100.0)],
                "saved Claude window priority must not depend on usage or response order: {payload}"
            );
        }
    }
}

#[test]
fn saved_codex_reads_scoped_quota_without_starting_a_cli() {
    let (url, request) = serve_once_capturing(
        "200 OK",
        &[],
        r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":42,"limit_window_seconds":18000,"reset_at":1800000000},"secondary_window":{"used_percent":73,"limit_window_seconds":604800}},"additional_rate_limits":[{"limit_name":"Extra model","metered_feature":"model-a","rate_limit":{"primary_window":{"used_percent":15,"limit_window_seconds":18000}}}],"credits":{"has_credits":true,"unlimited":false,"balance":"12"}}"#,
    );
    let dto = read_at(
        &identity(AgentId::Codex),
        &json!({"tokens":{"access_token":"fixture-access"}}),
        "unused",
        "unused",
        &url,
        "unused",
    )
    .unwrap();
    let request = request.join().unwrap();
    assert!(request
        .head
        .to_lowercase()
        .contains("chatgpt-account-id: team"));
    assert!(request.head.contains("Bearer fixture-access"));
    assert_eq!(dto.plan.as_deref(), Some("pro"));
    assert_eq!(dto.windows.len(), 3);
    let session = dto.windows.iter().find(|w| w.id == "primary").unwrap();
    let weekly = dto.windows.iter().find(|w| w.id == "secondary").unwrap();
    assert_eq!(session.used_percent, 42.0);
    assert_eq!(session.window_seconds, Some(18000));
    assert_eq!(weekly.used_percent, 73.0);
    assert_eq!(dto.windows[2].kind, crate::dto::LimitWindowKind::Model);
    assert_eq!(dto.credits.unwrap().balance, "12");
    assert!(!dto.current_account);
    assert!(dto.reset_offer.is_none());
    assert_eq!(
        dto.reset_credits, None,
        "no count in the body is unknown, not zero"
    );
}

/// A business member's usage body carries the workspace-credit share as `spend_control`, in
/// seconds and snake_case like the rest of it.
#[test]
fn saved_codex_reads_the_members_share_of_the_workspace_credits() {
    let (url, request) = serve_once_capturing(
        "200 OK",
        &[],
        r#"{"plan_type":"self_serve_business_prolite","rate_limit":{"primary_window":{"used_percent":12,"limit_window_seconds":18000}},"credits":{"has_credits":true,"unlimited":false,"balance":"0"},"spend_control":{"reached":false,"individual_limit":{"source":"workspace","limit":"25000","used":"8000","remaining":"17000","used_percent":32,"remaining_percent":68,"reset_after_seconds":3600,"reset_at":1790000000}}}"#,
    );
    let dto = read_at(
        &identity(AgentId::Codex),
        &json!({"tokens":{"access_token":"fixture-access"}}),
        "unused",
        "unused",
        &url,
        "unused",
    )
    .unwrap();
    request.join().unwrap();

    assert_eq!(
        dto.workspace_credits,
        Some(crate::dto::LimitsWorkspaceCreditsDto {
            limit: "25000".to_string(),
            used: "8000".to_string(),
            used_percent: 32.0,
            resets_at: Some("2026-09-21T14:13:20+00:00".to_string()),
            reached: false,
        })
    );
}

/// A saved account's meter is Codex's own too: the usage body's `remaining_percent`.
#[test]
fn saved_codex_takes_the_shares_meter_from_what_codex_says_remains() {
    let (url, request) = serve_once_capturing(
        "200 OK",
        &[],
        r#"{"plan_type":"self_serve_business_prolite","rate_limit":{"primary_window":{"used_percent":12,"limit_window_seconds":18000}},"spend_control":{"reached":false,"individual_limit":{"source":"workspace","limit":"25000","used":"8123","remaining":"16877","used_percent":32,"remaining_percent":68,"reset_after_seconds":3600,"reset_at":1790000000}}}"#,
    );
    let dto = read_at(
        &identity(AgentId::Codex),
        &json!({"tokens":{"access_token":"fixture-access"}}),
        "unused",
        "unused",
        &url,
        "unused",
    )
    .unwrap();
    request.join().unwrap();

    assert_eq!(dto.workspace_credits.expect("a share").used_percent, 32.0);
}

/// A saved member at their cap reads as used up, which only `spend_control.reached` says.
#[test]
fn saved_codex_marks_a_members_used_up_share_reached() {
    let (url, request) = serve_once_capturing(
        "200 OK",
        &[],
        r#"{"plan_type":"self_serve_business_prolite","rate_limit":{"primary_window":{"used_percent":12,"limit_window_seconds":18000}},"spend_control":{"reached":true,"individual_limit":{"source":"workspace","limit":"25000","used":"25000","remaining":"0","used_percent":100,"remaining_percent":0,"reset_after_seconds":3600,"reset_at":1790000000}}}"#,
    );
    let dto = read_at(
        &identity(AgentId::Codex),
        &json!({"tokens":{"access_token":"fixture-access"}}),
        "unused",
        "unused",
        &url,
        "unused",
    )
    .unwrap();
    request.join().unwrap();

    let share = dto.workspace_credits.expect("a share");
    assert_eq!(share.used, "25000");
    assert!(share.reached);
}

const CODEX_USAGE_WITH_RESETS: &str = r#"{"rate_limit":{"primary_window":{"used_percent":42,"limit_window_seconds":18000}},"rate_limit_reset_credits":{"available_count":2}}"#;

/// Two endpoints on one loopback server, which answers in request order whatever the path.
fn codex_endpoints(
    responses: &[(&str, &[&str], &str)],
) -> (
    String,
    String,
    std::thread::JoinHandle<Vec<crate::http::CapturedRequest>>,
) {
    let (url, requests) = crate::http::serve_sequence(responses);
    let base = url.trim_end_matches("/graphql").to_owned();
    (
        format!("{base}/wham/usage"),
        format!("{base}/wham/rate-limit-reset-credits"),
        requests,
    )
}

fn read_codex(usage: &str, resets: &str) -> Result<ProviderLimitsDto, HttpError> {
    read_at(
        &identity(AgentId::Codex),
        &json!({"tokens":{"access_token":"fixture-access"}}),
        "unused",
        "unused",
        usage,
        resets,
    )
}

#[test]
fn saved_codex_reads_the_banked_reset_count_and_the_soonest_expiry_of_an_available_one() {
    let (usage, resets, requests) = codex_endpoints(&[
        ("200 OK", &[], CODEX_USAGE_WITH_RESETS),
        (
            "200 OK",
            &[],
            r#"{"available_count":3,"total_earned_count":4,"credits":[
                {"id":"a","reset_type":"codex_rate_limits","status":"available","granted_at":"2026-09-01T00:00:00Z","expires_at":"2026-10-20T00:00:00Z"},
                {"id":"b","reset_type":"codex_rate_limits","status":"redeemed","granted_at":"2026-09-01T00:00:00Z","expires_at":"2026-09-30T00:00:00Z"},
                {"id":"c","reset_type":"codex_rate_limits","status":"available","granted_at":"2026-09-02T00:00:00+00:00","expires_at":"2026-10-10T12:00:00+00:00"},
                {"id":"d","reset_type":"codex_rate_limits","status":"available","granted_at":"2026-09-03T00:00:00Z","expires_at":null},
                {"id":"e","reset_type":"codex_rate_limits","status":"available","granted_at":"2026-09-03T00:00:00Z"}]}"#,
        ),
    ]);
    let dto = read_codex(&usage, &resets).unwrap();
    let requests = requests.join().unwrap();

    let detail = &requests[1].head;
    assert!(
        detail
            .lines()
            .next()
            .unwrap_or_default()
            .contains("/wham/rate-limit-reset-credits "),
        "{detail}"
    );
    assert!(detail.contains("Bearer fixture-access"));
    assert!(detail.to_lowercase().contains("chatgpt-account-id: team"));
    // The detail read answered in full, so its count wins over the usage body's 2, as in Codex.
    assert_eq!(
        dto.reset_credits,
        Some(LimitsResetCreditsDto {
            available_count: 3,
            next_expires_at: Some("2026-10-10T12:00:00+00:00".to_owned()),
        })
    );
    assert_eq!(dto.windows[0].used_percent, 42.0);
}

/// Codex's own app-server keeps the usage body's count when the detail read fails, and one credit
/// it cannot read fails the whole detail read; so does this.
#[test]
fn saved_codex_keeps_the_count_when_the_expiry_read_fails() {
    for detail in [
        ("500 Internal Server Error", "{}"),
        (
            "200 OK",
            r#"{"available_count":5,"credits":[
                {"status":"available","expires_at":"2026-10-10T12:00:00Z"},
                {"status":"available","expires_at":"not a date"}]}"#,
        ),
    ] {
        let (usage, resets, requests) = codex_endpoints(&[
            ("200 OK", &[], CODEX_USAGE_WITH_RESETS),
            (detail.0, &[], detail.1),
        ]);
        let dto = read_codex(&usage, &resets).unwrap();
        requests.join().unwrap();

        assert_eq!(
            dto.reset_credits,
            Some(LimitsResetCreditsDto {
                available_count: 2,
                next_expires_at: None,
            }),
            "{detail:?}"
        );
        assert_eq!(dto.windows[0].used_percent, 42.0);
    }
}

#[test]
fn saved_codex_reports_zero_banked_resets_without_asking_for_their_detail() {
    let (usage, _, requests) = codex_endpoints(&[(
        "200 OK",
        &[],
        r#"{"rate_limit":{"primary_window":{"used_percent":42}},"rate_limit_reset_credits":{"available_count":0}}"#,
    )]);
    // A listener that never answers: a detail request would sit in its backlog, where `accept`
    // finds it, rather than being refused and swallowed like a failed detail read.
    let detail = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    detail.set_nonblocking(true).unwrap();
    let resets = format!(
        "http://{}/wham/rate-limit-reset-credits",
        detail.local_addr().unwrap()
    );
    let dto = read_codex(&usage, &resets).unwrap();
    requests.join().unwrap();

    assert_eq!(
        detail.accept().map(|_| ()).map_err(|error| error.kind()),
        Err(std::io::ErrorKind::WouldBlock),
        "a count of 0 has no detail worth a request"
    );

    assert_eq!(
        dto.reset_credits,
        Some(LimitsResetCreditsDto {
            available_count: 0,
            next_expires_at: None,
        })
    );
}

/// A count Codex's own client would refuse leaves the resets unknown and never costs the windows.
#[test]
fn saved_codex_treats_a_malformed_banked_reset_count_as_unknown() {
    for count in ["-1", "1.5", "\"2\"", "null"] {
        let body = format!(
            r#"{{"rate_limit":{{"primary_window":{{"used_percent":42}}}},"rate_limit_reset_credits":{{"available_count":{count}}}}}"#
        );
        let (usage, resets, requests) = codex_endpoints(&[("200 OK", &[], &body)]);
        let dto = read_codex(&usage, &resets).unwrap();
        requests.join().unwrap();

        assert_eq!(dto.reset_credits, None, "{count}");
        assert_eq!(dto.windows[0].used_percent, 42.0, "{count}");
    }
}

#[test]
fn wrong_claude_identity_stops_before_usage() {
    let (profile, p) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"other"},"organization":{"uuid":"team"}}"#,
    );
    let result = read_at(
        &identity(AgentId::Claude),
        &json!({"claudeAiOauth":{"accessToken":"fixture"}}),
        &profile,
        &crate::http::refused_url(),
        "unused",
        "unused",
    );
    p.join().unwrap();
    assert!(matches!(result, Err(HttpError::Unauthorized)));
}

#[test]
fn matching_claude_user_in_another_workspace_is_rejected_before_usage() {
    let (url, request) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"user"},"organization":{"uuid":"other-team"}}"#,
    );
    let result = read_at(
        &identity(AgentId::Claude),
        &json!({"claudeAiOauth":{"accessToken":"fixture"}}),
        &url,
        &crate::http::refused_url(),
        "unused",
        "unused",
    );
    request.join().unwrap();
    assert!(matches!(result, Err(HttpError::Unauthorized)));
}

#[test]
fn codex_quota_for_another_account_is_rejected() {
    let (url, request) = serve_once(
        "200 OK",
        r#"{"account_id":"other-team","rate_limit":{"primary_window":{"used_percent":42}}}"#,
    );
    let result = read_at(
        &identity(AgentId::Codex),
        &json!({"tokens":{"access_token":"fixture"}}),
        "unused",
        "unused",
        &url,
        "unused",
    );
    request.join().unwrap();
    assert!(matches!(result, Err(HttpError::Unauthorized)));
}
