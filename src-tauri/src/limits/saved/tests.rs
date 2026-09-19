use super::*;
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
    );
    request.join().unwrap();
    assert!(matches!(result, Err(HttpError::Unauthorized)));
}
