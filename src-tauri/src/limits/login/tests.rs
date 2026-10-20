use super::*;

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
    assert_eq!(
        std::fs::read_dir(home.path().join(".on-n-off/limits"))
            .unwrap()
            .count(),
        3
    );
}
