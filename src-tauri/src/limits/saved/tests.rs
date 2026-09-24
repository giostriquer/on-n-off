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
        CodexEndpoints {
            usage: "unused",
            reset_credits: "unused",
            credit_usage: "unused",
        },
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
        CodexEndpoints {
            usage: "unused",
            reset_credits: "unused",
            credit_usage: "unused",
        },
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
        CodexEndpoints {
            usage: "unused",
            reset_credits: "unused",
            credit_usage: "unused",
        },
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
                CodexEndpoints {
                    usage: "unused",
                    reset_credits: "unused",
                    credit_usage: "unused",
                },
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
        CodexEndpoints {
            usage: &url,
            reset_credits: "unused",
            credit_usage: "unused",
        },
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
        CodexEndpoints {
            usage: &url,
            reset_credits: "unused",
            credit_usage: "unused",
        },
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
        CodexEndpoints {
            usage: &url,
            reset_credits: "unused",
            credit_usage: "unused",
        },
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
        CodexEndpoints {
            usage: &url,
            reset_credits: "unused",
            credit_usage: "unused",
        },
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
        CodexEndpoints {
            usage,
            reset_credits: resets,
            credit_usage: "unused",
        },
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
        CodexEndpoints {
            usage: "unused",
            reset_credits: "unused",
            credit_usage: "unused",
        },
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
        CodexEndpoints {
            usage: "unused",
            reset_credits: "unused",
            credit_usage: "unused",
        },
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
        CodexEndpoints {
            usage: &url,
            reset_credits: "unused",
            credit_usage: "unused",
        },
    );
    request.join().unwrap();
    assert!(matches!(result, Err(HttpError::Unauthorized)));
}

const CODEX_BUSINESS_USAGE: &str = r#"{"plan_type":"self_serve_business_prolite","rate_limit":{"primary_window":{"used_percent":12,"limit_window_seconds":604800}},"credits":{"has_credits":true,"unlimited":false,"balance":"0"}}"#;

/// A daily breakdown in the shape the per-member endpoint answers, dated back from today (UTC).
fn spending(days: &[(u64, &[f64])]) -> String {
    let today = chrono::Utc::now().date_naive();
    json!({
        "data": days.iter().map(|(back, credits)| json!({
            "date": (today - chrono::Days::new(*back)).format("%Y-%m-%d").to_string(),
            "product_surface_usage_values": {"cli": 1.0},
            "premium_usage_values": {"total_usage_credits": {}, "credit_usage_credits": {}},
            "models": credits.iter().map(|credits| json!({"model": "model-a", "credits": credits})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "units": "credits",
        "data_freshness_ts": "2026-09-24T19:00:00Z",
        "group_by": "day",
    })
    .to_string()
}

/// A saved Codex member of workspace `team`. Each test names its own member, because a failed
/// spending read backs off per account and must not hold another test's read back.
fn member(user: &str) -> Identity {
    Identity {
        provider: AgentId::Codex,
        user_id: format!("{user}:{:?}", std::thread::current().id()),
        workspace_id: "team".into(),
    }
}

fn read_codex_spending(
    who: &Identity,
    usage: &str,
    spending: &str,
) -> Result<ProviderLimitsDto, HttpError> {
    read_at(
        who,
        &json!({"tokens":{"access_token":"fixture-access"}}),
        "unused",
        "unused",
        CodexEndpoints {
            usage,
            reset_credits: "unused",
            credit_usage: spending,
        },
    )
}

/// A business member's card has no credit figure but spending: the per-member daily breakdown the
/// Codex app's usage history reads, for the 30 UTC days up to today, with the same token.
#[test]
fn saved_codex_reads_what_a_workspace_member_spent() {
    let breakdown = spending(&[(0, &[100.5, 20.0]), (3, &[50.0]), (20, &[1000.0])]);
    let (url, requests) = crate::http::serve_sequence(&[
        ("200 OK", &[], CODEX_BUSINESS_USAGE),
        ("200 OK", &[], &breakdown),
    ]);
    let base = url.trim_end_matches("/graphql");
    // The read dates its window by the clock; a run that crosses UTC midnight may see either day.
    let before = chrono::Utc::now().date_naive();
    let dto = read_codex_spending(
        &member("spent"),
        &format!("{base}/wham/usage"),
        &format!("{base}/wham/usage/daily-workspace-user-token-usage-breakdown"),
    )
    .unwrap();
    let after = chrono::Utc::now().date_naive();
    let requests = requests.join().unwrap();

    let asked = requests[1]
        .head
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    let window = |today: chrono::NaiveDate| {
        format!(
            "/wham/usage/daily-workspace-user-token-usage-breakdown?start_date={}&end_date={}&group_by=day ",
            (today - chrono::Days::new(29)).format("%Y-%m-%d"),
            today.format("%Y-%m-%d"),
        )
    };
    assert!(
        asked.contains(&window(before)) || asked.contains(&window(after)),
        "{asked}"
    );
    assert!(requests[1].head.contains("Bearer fixture-access"));
    assert!(requests[1]
        .head
        .to_lowercase()
        .contains("chatgpt-account-id: team"));
    assert_eq!(
        dto.credits_spent,
        Some(crate::dto::LimitsCreditsSpentDto {
            last_7_days: 170.5,
            last_30_days: 1170.5,
            updated_at: Some("2026-09-24T19:00:00Z".to_string()),
        })
    );
    assert_eq!(dto.windows[0].used_percent, 12.0);
}

/// Only a workspace pools credits, so a personal plan is never asked what it spent.
#[test]
fn saved_codex_never_asks_a_personal_plan_what_it_spent() {
    let (url, requests) = crate::http::serve_sequence(&[(
        "200 OK",
        &[],
        r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":12}}}"#,
    )]);
    let spending = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    spending.set_nonblocking(true).unwrap();
    let dto = read_codex_spending(
        &member("personal"),
        &format!("{}/wham/usage", url.trim_end_matches("/graphql")),
        &format!("http://{}/breakdown", spending.local_addr().unwrap()),
    )
    .unwrap();
    requests.join().unwrap();

    assert_eq!(
        spending.accept().map(|_| ()).map_err(|error| error.kind()),
        Err(std::io::ErrorKind::WouldBlock),
        "a personal plan has no pooled credits to ask about"
    );
    assert_eq!(dto.credits_spent, None);
}

/// As with the banked-reset detail, the spending read never decides the usage read: a member the
/// endpoint refuses, or an answer that is not counted in credits, only leaves the figure out.
#[test]
fn a_spending_read_that_fails_leaves_the_usage_read_standing() {
    let not_credits =
        spending(&[(0, &[5.0])]).replace(r#""units":"credits""#, r#""units":"tokens""#);
    for (status, body) in [
        ("403 Forbidden", "{}"),
        ("500 Internal Server Error", "{}"),
        ("200 OK", not_credits.as_str()),
    ] {
        let (url, requests) = crate::http::serve_sequence(&[
            ("200 OK", &[], CODEX_BUSINESS_USAGE),
            (status, &[], body),
        ]);
        let base = url.trim_end_matches("/graphql");
        let dto = read_codex_spending(
            &member(status),
            &format!("{base}/wham/usage"),
            &format!("{base}/breakdown"),
        )
        .unwrap();
        requests.join().unwrap();

        assert_eq!(dto.credits_spent, None, "{status}");
        assert_eq!(dto.windows[0].used_percent, 12.0, "{status}");
    }
}

/// "team" and "enterprise" are Claude plans too. The spending read belongs to Codex's usage read, so
/// a saved Claude account on either plan never asks the ChatGPT backend what it spent.
#[test]
fn a_saved_claude_team_or_enterprise_account_is_never_asked_what_it_spent() {
    for plan in ["team", "enterprise"] {
        let (profile, p) = serve_once(
            "200 OK",
            r#"{"account":{"uuid":"user","email":"you@example.com"},"organization":{"uuid":"team"}}"#,
        );
        let (usage, u) = serve_once("200 OK", r#"{"seven_day":{"utilization":61}}"#);
        let spending = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        spending.set_nonblocking(true).unwrap();
        let credit_usage = format!("http://{}/breakdown", spending.local_addr().unwrap());
        let dto = read_at(
            &identity(AgentId::Claude),
            &json!({"claudeAiOauth":{"accessToken":"fixture-access","subscriptionType":plan}}),
            &profile,
            &usage,
            CodexEndpoints {
                usage: "unused",
                reset_credits: "unused",
                credit_usage: &credit_usage,
            },
        )
        .unwrap();
        p.join().unwrap();
        u.join().unwrap();

        assert_eq!(dto.plan.as_deref(), Some(plan));
        assert_eq!(
            spending.accept().map(|_| ()).map_err(|error| error.kind()),
            Err(std::io::ErrorKind::WouldBlock),
            "a Claude {plan} account asked what it spent"
        );
        assert_eq!(dto.credits_spent, None);
    }
}
