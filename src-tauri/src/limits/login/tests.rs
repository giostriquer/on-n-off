use super::*;

fn identity() -> Identity {
    Identity {
        provider: AgentId::Codex,
        user_id: "user".into(),
        workspace_id: "team".into(),
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
    let who = identity();
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
