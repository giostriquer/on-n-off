use super::*;
use crate::http::serve_once;
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
    let dto = read_saved_claude_at(
        &identity(),
        Some(credential()),
        Utc::now().timestamp_millis(),
        &profile,
        &usage,
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
        let dto = read_saved_claude_at(
            &identity(),
            Some(credential()),
            Utc::now().timestamp_millis(),
            &profile,
            &usage,
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
        assert!(read_saved_claude_at(
            &identity(),
            Some(credential()),
            Utc::now().timestamp_millis(),
            &profile,
            &crate::http::refused_url()
        )
        .is_err());
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
    remember(home.path(), legacy.clone()).saved.unwrap();
    let mut unrelated = legacy.clone();
    unrelated.account.as_mut().unwrap().id = "other-team".into();
    remember(home.path(), unrelated.clone()).saved.unwrap();
    let mut fresh = legacy.clone();
    fresh.account.as_mut().unwrap().id = "profile:verified-user-team".into();
    fresh.account.as_mut().unwrap().legacy_id = Some("team".into());
    fresh.reading.windows[0].used_percent = 42.0;
    fresh.reading.windows[0].observed_at = "2026-09-13T12:00:00Z".into();
    remember(home.path(), fresh.clone()).saved.unwrap();
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

/// What a first usage reading keeps: an answer about the signed-in identity that observed
/// something, as a card that is not the signed-in account's. Anything else is no reading.
#[test]
fn a_first_reading_is_kept_only_when_it_answered_for_the_identity_with_something_observed() {
    let who = Identity {
        provider: AgentId::Codex,
        ..identity()
    };
    let answered = ProviderLimitsDto::for_test(AgentId::Codex, &who.observation_key())
        .with_reading(Reading {
            windows: vec![super::super::json::window(
                "primary",
                "Weekly · all models",
                crate::dto::LimitWindowKind::Weekly,
                42.0,
                None,
            )],
            ..Reading::default()
        });

    let kept = accepted(&who, answered.clone()).expect("an answer about the identity");
    assert!(!kept.current_account);
    assert_eq!(kept.reading, answered.reading);

    let failed = ProviderLimitsDto {
        status: LimitsStatus::Failed,
        ..answered.clone()
    };
    let mut another = answered.clone();
    another.account.as_mut().unwrap().id = "profile:someone-else".into();
    let unnamed = ProviderLimitsDto {
        account: None,
        ..answered.clone()
    };
    let empty = answered.with_reading(Reading::default());
    for (case, card) in [
        ("failed", failed),
        ("another account", another),
        ("no account", unnamed),
        ("nothing observed", empty),
    ] {
        assert_eq!(accepted(&who, card), None, "{case}");
    }
}
