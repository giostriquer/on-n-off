//! A saved profile's Claude read (`read_saved_claude`): its profile's user in its workspace, read
//! with the credential its login holds.
use super::*;
use crate::http::serve_once_capturing;
use serde_json::Value;

fn identity(provider: AgentId) -> Identity {
    Identity {
        provider,
        user_id: "user".into(),
        workspace_id: "team".into(),
    }
}

/// Where a read of these tests asks: `profile` and `usage` for Claude, nowhere for Codex.
fn urls<'a>(profile: &'a str, usage: &'a str) -> SavedReadUrls<'a> {
    SavedReadUrls {
        claude_profile: profile,
        claude_usage: usage,
        codex: CodexEndpoints {
            usage: "unused",
            reset_credits: "unused",
            credit_usage: "unused",
            subscriptions: "unused",
        },
    }
}

/// `read_saved_claude` with the credential in `auth`, a Claude credentials document.
fn read_at(
    identity: &Identity,
    auth: &Value,
    profile: &str,
    usage: &str,
) -> Result<ProviderLimitsDto, SavedReadError> {
    let credential = credentials::parse_claude_credential(auth).expect("the fixture's credential");
    read_saved_claude(identity, credential, &urls(profile, usage))
}

#[test]
fn saved_claude_reads_verified_usage_without_a_native_login() {
    let (profile, p) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"user","email":"you@example.com"},"organization":{"uuid":"team"}}"#,
    );
    let (usage, u) = serve_once_capturing("200 OK", &[], r#"{"seven_day":{"utilization":61}}"#);
    let auth = json!({"claudeAiOauth":{"accessToken":"fixture-access"}});
    let dto = read_at(&identity(AgentId::Claude), &auth, &profile, &usage).unwrap();
    p.join().unwrap();
    let request = u.join().unwrap();
    assert!(request.head.contains("Bearer fixture-access"));
    assert_eq!(dto.reading.windows[0].used_percent, 61.0);
    assert!(!dto.reading.windows[0].observed_at.is_empty());
    assert!(!dto.current_account);
    assert_eq!(
        dto.account.unwrap().id,
        identity(AgentId::Claude).observation_key()
    );
}

#[test]
fn saved_claude_sends_the_claude_headers_on_both_requests() {
    let (profile, p) = serve_once_capturing(
        "200 OK",
        &[],
        r#"{"account":{"uuid":"user"},"organization":{"uuid":"team"}}"#,
    );
    let (usage, u) = serve_once_capturing("200 OK", &[], r#"{"seven_day":{"utilization":61}}"#);
    read_at(
        &identity(AgentId::Claude),
        &json!({"claudeAiOauth":{"accessToken":"fixture-access"}}),
        &profile,
        &usage,
    )
    .unwrap();
    for head in [p.join().unwrap().head, u.join().unwrap().head] {
        super::assert_claude_headers(&head, "Bearer fixture-access");
    }
}

/// A saved Claude account carries the subscription status its profile reports, like the native one.
#[test]
fn saved_claude_carries_the_subscription_status_its_profile_reports() {
    let (profile, p) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"user","email":"you@example.com"},"organization":{"uuid":"team","subscription_status":"canceled"}}"#,
    );
    let (usage, u) = serve_once_capturing("200 OK", &[], r#"{"seven_day":{"utilization":61}}"#);
    let auth = json!({"claudeAiOauth":{"accessToken":"fixture-access"}});

    let dto = read_at(&identity(AgentId::Claude), &auth, &profile, &usage).unwrap();
    p.join().unwrap();
    u.join().unwrap();

    assert_eq!(dto.reading.subscription_status.as_deref(), Some("canceled"));
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
    let dto = read_at(&identity(AgentId::Claude), &auth, &profile, &usage).unwrap();
    p.join().unwrap();
    let request = u.join().unwrap();
    let request_line = request.head.lines().next().unwrap_or_default();
    assert!(
        request_line.contains("?cedar_ember=1&skip_spend=1 "),
        "{request_line}"
    );
    assert_eq!(dto.reading.windows[0].used_percent, 61.0);
    assert_eq!(
        dto.reading
            .reset_credits
            .map(|resets| resets.available_count),
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
    assert_eq!(dto.reading.windows[0].used_percent, 61.0);
    assert_eq!(dto.reading.reset_credits, None);
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
            )
            .unwrap();
            p.join().unwrap();
            u.join().unwrap();

            assert!(!dto.current_account);
            assert_eq!(
                dto.reading.windows
                    .iter()
                    .map(|window| (window.kind, window.used_percent))
                    .collect::<Vec<_>>(),
                vec![(Weekly, 99.0), (Session, f64::from(session_percent)), (Model, 100.0)],
                "saved Claude window priority must not depend on usage or response order: {payload}"
            );
        }
    }
}

/// A login that now signs in as another user is its own outcome, found before usage is asked.
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
    );
    p.join().unwrap();
    assert_eq!(result.err(), Some(SavedReadError::OtherAccount));
}

/// The same user in another workspace is another account too.
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
    );
    request.join().unwrap();
    assert_eq!(result.err(), Some(SavedReadError::OtherAccount));
}

/// "team" and "enterprise" are Claude plans too. The spending read belongs to Codex's usage read, so
/// a saved Claude account on either plan has no spending figure.
#[test]
fn a_saved_claude_team_or_enterprise_account_has_no_spending_figure() {
    for plan in ["team", "enterprise"] {
        let (profile, p) = serve_once(
            "200 OK",
            r#"{"account":{"uuid":"user","email":"you@example.com"},"organization":{"uuid":"team"}}"#,
        );
        let (usage, u) = serve_once("200 OK", r#"{"seven_day":{"utilization":61}}"#);
        let dto = read_at(
            &identity(AgentId::Claude),
            &json!({"claudeAiOauth":{"accessToken":"fixture-access","subscriptionType":plan}}),
            &profile,
            &usage,
        )
        .unwrap();
        p.join().unwrap();
        u.join().unwrap();

        assert_eq!(dto.reading.plan.as_deref(), Some(plan));
        assert_eq!(dto.reading.credits_spent, None);
    }
}

/// A refused login is `Unauthorized`, and a throttled one is rate limited until its `Retry-After`.
#[test]
fn a_saved_claude_read_the_service_refuses_or_throttles_says_which() {
    for (status, expected) in [
        ("401 Unauthorized", HttpError::Unauthorized),
        (
            "429 Too Many Requests",
            HttpError::RateLimited(crate::http::RateLimitReset::RetryAfter(30)),
        ),
    ] {
        let (profile, p) = serve_once_capturing(status, &["Retry-After: 30"], "{}");
        let claude = read_at(
            &identity(AgentId::Claude),
            &json!({"claudeAiOauth":{"accessToken":"fixture-access"}}),
            &profile,
            &crate::http::refused_url(),
        );
        p.join().unwrap();
        assert_eq!(claude.err(), Some(expected.into()), "{status}");
    }
}

/// A read that answered with nothing to show is an error, so the card keeps what it remembers.
#[test]
fn a_saved_claude_read_that_observed_nothing_is_an_error() {
    let (profile, p) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"user"},"organization":{"uuid":"team"}}"#,
    );
    let (usage, u) = serve_once("200 OK", "{}");
    let claude = read_at(
        &identity(AgentId::Claude),
        &json!({"claudeAiOauth":{"accessToken":"fixture-access"}}),
        &profile,
        &usage,
    );
    p.join().unwrap();
    u.join().unwrap();
    assert_eq!(
        claude.err(),
        Some(HttpError::Parse("Usage response contained no quota observations.".into()).into())
    );
}

/// A Max ×20 login's credential, as an isolated sign-in leaves it.
fn max_credential() -> ClaudeCredential {
    credentials::parse_claude_credential(&json!({"claudeAiOauth": {
        "accessToken":"fixture-access", "subscriptionType":"max", "rateLimitTier":"default_claude_max_20x"
    }}))
    .unwrap()
}

#[test]
fn a_first_claude_reading_is_remembered_as_a_scoped_dated_snapshot() {
    let home = tempfile::tempdir().unwrap();
    let (profile, profile_request) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"user","email":"me@example.com"},"organization":{"uuid":"team"}}"#,
    );
    let (usage, usage_request) = serve_once(
        "200 OK",
        r#"{"seven_day":{"utilization":61,"resets_at":"2026-09-20T12:00:00Z"}}"#,
    );
    let dto = read_saved_claude(
        &identity(AgentId::Claude),
        max_credential(),
        &urls(&profile, &usage),
    )
    .unwrap();
    profile_request.join().unwrap();
    usage_request.join().unwrap();
    assert!(!dto.current_account);
    assert_eq!(dto.reading.plan.as_deref(), Some("max ×20"));
    assert_eq!(dto.reading.windows[0].used_percent, 61.0);
    assert!(!dto.reading.windows[0].observed_at.is_empty());
    let account = dto.account.as_ref().unwrap();
    assert!(account.id.starts_with("profile:"));
    assert_eq!(account.legacy_id.as_deref(), Some("user"));
    assert_eq!(account.label.as_deref(), Some("me@example.com"));
    remember(home.path(), dto.clone()).saved.unwrap();
    let reloaded = SnapshotStore::for_home(home.path()).load(AgentId::Claude);
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].reading.windows, dto.reading.windows);
    assert_eq!(reloaded[0].account, dto.account);
}

#[test]
fn saved_claude_session_keeps_the_reported_percentage_and_optional_reset() {
    for reset in [None, Some("2026-09-20T12:00:00Z")] {
        let home = tempfile::tempdir().unwrap();
        let (profile, profile_request) = serve_once(
            "200 OK",
            r#"{"account":{"uuid":"user","email":"me@example.com"},"organization":{"uuid":"team"}}"#,
        );
        let body = json!({"limits": [
            {"kind":"session", "group":"session", "percent":0, "resets_at":reset},
            {"kind":"weekly_all", "group":"weekly", "percent":90, "resets_at":"2026-09-21T12:00:00Z"}
        ]}).to_string();
        let (usage, usage_request) = serve_once("200 OK", &body);
        let dto = read_saved_claude(
            &identity(AgentId::Claude),
            max_credential(),
            &urls(&profile, &usage),
        )
        .unwrap();
        profile_request.join().unwrap();
        usage_request.join().unwrap();
        remember(home.path(), dto.clone()).saved.unwrap();
        let saved = SnapshotStore::for_home(home.path()).load(AgentId::Claude);
        let session = saved[0]
            .reading
            .windows
            .iter()
            .find(|w| w.id == "session")
            .unwrap();
        assert_eq!(session.used_percent, 0.0);
        assert_eq!(session.resets_at.as_deref(), reset);
    }
}

#[test]
fn wrong_claude_user_or_workspace_never_contributes_usage() {
    for body in [
        r#"{"account":{"uuid":"other","email":"me@example.com"},"organization":{"uuid":"team"}}"#,
        r#"{"account":{"uuid":"user","email":"me@example.com"},"organization":{"uuid":"other"}}"#,
    ] {
        let home = tempfile::tempdir().unwrap();
        let (profile, request) = serve_once("200 OK", body);
        assert!(read_saved_claude(
            &identity(AgentId::Claude),
            max_credential(),
            &urls(&profile, &crate::http::refused_url()),
        )
        .is_err());
        request.join().unwrap();
        assert!(SnapshotStore::for_home(home.path())
            .load(AgentId::Claude)
            .is_empty());
    }
}
