use super::*;
use crate::http::{head_header, serve_once};
use serde_json::json;

fn identity() -> Identity {
    Identity {
        provider: AgentId::Claude,
        user_id: "user".into(),
        workspace_id: "team".into(),
    }
}
fn credential() -> ClaudeCredential {
    credentials::parse_claude_credential(&json!({"claudeAiOauth": {
        "accessToken":"fixture-access", "subscriptionType":"max", "rateLimitTier":"default_claude_max_20x"
    }})).unwrap()
}
#[test]
fn isolated_claude_sign_in_keeps_a_scoped_dated_snapshot_without_reading_the_active_home() {
    let home = tempfile::tempdir().unwrap();
    let (profile, profile_request) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"user","email":"me@example.com"},"organization":{"uuid":"team"}}"#,
    );
    let (usage, usage_request) = serve_once(
        "200 OK",
        r#"{"seven_day":{"utilization":61,"resets_at":"2026-09-20T12:00:00Z"}}"#,
    );
    let dto = read_claude_at(&identity(), credential(), &profile, &usage).unwrap();
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
    remember(home.path(), &dto).unwrap();
    let reloaded = SnapshotStore::for_home(home.path()).load(AgentId::Claude);
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].reading.windows, dto.reading.windows);
    assert_eq!(reloaded[0].account, dto.account);
    assert!(!home.path().join(".claude").exists());
    assert!(!home.path().join(".claude.json").exists());
}
/// The first usage read sends the signed-in read's headers.
#[test]
fn the_first_usage_read_sends_the_signed_in_reads_headers() {
    let (profile, profile_request) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"user","email":"me@example.com"},"organization":{"uuid":"team"}}"#,
    );
    let (usage, usage_request) = serve_once("200 OK", r#"{"seven_day":{"utilization":61}}"#);
    read_claude_at(&identity(), credential(), &profile, &usage).unwrap();
    let profile_head = profile_request.join().unwrap();
    let usage_head = usage_request.join().unwrap();
    for head in [&profile_head, &usage_head] {
        assert_eq!(
            head_header(head, "authorization"),
            Some("Bearer fixture-access"),
            "{head}"
        );
        assert_eq!(
            head_header(head, "cache-control"),
            Some("no-cache"),
            "{head}"
        );
    }
    assert_eq!(
        head_header(&profile_head, "content-type"),
        Some("application/json"),
        "{profile_head}"
    );
    assert_eq!(
        head_header(&profile_head, "anthropic-beta"),
        None,
        "{profile_head}"
    );
    assert_eq!(
        head_header(&usage_head, "anthropic-beta"),
        Some("oauth-2025-04-20"),
        "{usage_head}"
    );
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
        let dto = read_claude_at(&identity(), credential(), &profile, &usage).unwrap();
        profile_request.join().unwrap();
        usage_request.join().unwrap();
        remember(home.path(), &dto).unwrap();
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
        assert!(read_claude_at(
            &identity(),
            credential(),
            &profile,
            &crate::http::refused_url()
        )
        .is_none());
        request.join().unwrap();
        assert!(SnapshotStore::for_home(home.path())
            .load(AgentId::Claude)
            .is_empty());
    }
}
#[test]
fn a_new_sign_in_supersedes_only_matching_legacy_history_without_relabeling_its_windows() {
    let home = tempfile::tempdir().unwrap();
    let mut legacy = ProviderLimitsDto {
        current_account: false,
        ..ProviderLimitsDto::for_test(AgentId::Codex, "team")
            .labelled("me@example.com")
            .with_reading(Reading {
                plan: Some("pro".into()),
                windows: vec![super::super::json::window(
                    "primary",
                    "Weekly · all models",
                    crate::dto::LimitWindowKind::Weekly,
                    100.0,
                    None,
                )],
                ..Reading::default()
            })
    };
    legacy.reading.windows[0].observed_at = "2026-09-11T12:00:00Z".into();
    remember(home.path(), &legacy).unwrap();
    let mut unrelated = legacy.clone();
    unrelated.account.as_mut().unwrap().id = "other-team".into();
    remember(home.path(), &unrelated).unwrap();
    let mut fresh = legacy.clone();
    fresh.account.as_mut().unwrap().id = "profile:verified-user-team".into();
    fresh.account.as_mut().unwrap().legacy_id = Some("team".into());
    fresh.reading.windows[0].used_percent = 42.0;
    fresh.reading.windows[0].observed_at = "2026-09-13T12:00:00Z".into();
    remember(home.path(), &fresh).unwrap();
    let cards = SnapshotStore::for_home(home.path()).load(AgentId::Codex);
    assert_eq!(cards.len(), 2);
    assert_eq!(
        cards[0].account.as_ref().unwrap().id,
        "profile:verified-user-team"
    );
    assert_eq!(cards[0].reading.windows[0].used_percent, 42.0);
    assert_eq!(cards[1].account.as_ref().unwrap().id, "other-team");
    assert_eq!(cards[1].reading.windows[0].used_percent, 100.0);
    // The superseded original remains on disk, with its original identity and observations.
    assert_eq!(
        std::fs::read_dir(home.path().join(".on-n-off/limits"))
            .unwrap()
            .count(),
        3
    );
}
