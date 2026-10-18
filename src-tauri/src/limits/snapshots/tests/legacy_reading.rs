use super::*;

const LEGACY_SEEN: &str = "2026-10-16T21:40:44.166Z";
const SCOPED_SEEN: &str = "2026-10-18T06:30:00.000Z";

fn legacy(provider: AgentId, label: &str) -> ProviderLimitsDto {
    let mut dto = ProviderLimitsDto::for_test(provider, "email:you@example.com")
        .labelled(label)
        .with_reading(Reading {
            plan: Some("Muse Code Power Usage".to_string()),
            windows: vec![
                crate::limits::json::window(
                    "weekly",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    30.0,
                    Some("2026-10-19T00:00:00.000Z".to_string()),
                ),
                crate::limits::json::window(
                    "session",
                    "5 hour · all models",
                    LimitWindowKind::Session,
                    91.0,
                    Some("2026-10-16T21:45:29.000Z".to_string()),
                ),
            ],
            ..Reading::default()
        });
    for window in &mut dto.reading.windows {
        window.observed_at = LEGACY_SEEN.to_string();
    }
    dto
}

fn idle(provider: AgentId) -> ProviderLimitsDto {
    let mut dto = ProviderLimitsDto::for_test(provider, "profile:you")
        .labelled("You@Example.com")
        .with_legacy_id("email:you@example.com")
        .with_reading(Reading {
            plan: Some("Muse Code Power Usage".to_string()),
            windows: vec![crate::limits::json::window(
                "session",
                "5 hour · all models",
                LimitWindowKind::Session,
                0.0,
                None,
            )],
            ..Reading::default()
        });
    dto.reading.windows[0].observed_at = SCOPED_SEEN.to_string();
    dto
}

fn weekly(dto: &ProviderLimitsDto) -> Option<(f64, &str)> {
    dto.reading
        .windows
        .iter()
        .find(|window| window.kind == LimitWindowKind::Weekly)
        .map(|window| (window.used_percent, window.observed_at.as_str()))
}

#[test]
fn a_first_muse_reading_keeps_the_weekly_its_legacy_history_last_saw() {
    let home = scratch_dir("limits-legacy-weekly-muse");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&legacy(AgentId::Muse, "you@example.com"))
        .unwrap();

    let remembered = store.remember(idle(AgentId::Muse));

    remembered.saved.unwrap();
    assert_eq!(weekly(&remembered.card), Some((30.0, LEGACY_SEEN)));
    assert_eq!(
        remembered
            .card
            .reading
            .windows
            .iter()
            .map(|window| (window.kind, window.used_percent))
            .collect::<Vec<_>>(),
        [
            (LimitWindowKind::Weekly, 30.0),
            (LimitWindowKind::Session, 0.0)
        ]
    );
    let loaded = store.load(AgentId::Muse);
    assert_eq!(loaded.len(), 1, "the legacy card is superseded");
    assert_eq!(
        loaded[0]
            .account
            .as_ref()
            .map(|account| account.id.as_str()),
        Some("profile:you")
    );
    assert_eq!(weekly(&loaded[0]), Some((30.0, LEGACY_SEEN)));
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn claude_and_codex_scoped_readings_never_take_legacy_quotas() {
    for provider in [AgentId::Claude, AgentId::Codex] {
        let home = scratch_dir(&format!("limits-legacy-weekly-{}", provider.key()));
        let store = SnapshotStore::for_home(&home);
        store.save(&legacy(provider, "you@example.com")).unwrap();

        let remembered = store.remember(idle(provider));

        assert_eq!(weekly(&remembered.card), None, "{provider:?}");
        fs::remove_dir_all(home).unwrap();
    }
}

#[test]
fn legacy_history_under_another_email_is_not_carried() {
    let home = scratch_dir("limits-legacy-weekly-other-email");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&legacy(AgentId::Muse, "someone@example.com"))
        .unwrap();

    let remembered = store.remember(idle(AgentId::Muse));

    assert_eq!(weekly(&remembered.card), None);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_failed_first_reading_does_not_take_the_legacy_reading() {
    let home = scratch_dir("limits-legacy-weekly-failed");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&legacy(AgentId::Muse, "you@example.com"))
        .unwrap();
    let mut failed = idle(AgentId::Muse);
    failed.status = LimitsStatus::Failed;
    failed.reading = Reading::default();

    let remembered = store.remember(failed);

    assert!(remembered.card.reading.windows.is_empty());
    assert_eq!(remembered.card.reading.plan, None);
    assert_eq!(store.load(AgentId::Muse).len(), 1);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_scoped_history_of_its_own_wins_over_the_legacy_one() {
    let home = scratch_dir("limits-legacy-weekly-own-history");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&legacy(AgentId::Muse, "you@example.com"))
        .unwrap();
    let mut own = legacy(AgentId::Muse, "You@Example.com");
    own.account = idle(AgentId::Muse).account;
    own.reading.windows[0].used_percent = 50.0;
    for window in &mut own.reading.windows {
        window.observed_at = "2026-10-17T10:00:00.000Z".to_string();
    }
    store.save(&own).unwrap();

    let remembered = store.remember(idle(AgentId::Muse));

    assert_eq!(
        weekly(&remembered.card),
        Some((50.0, "2026-10-17T10:00:00.000Z"))
    );
    fs::remove_dir_all(home).unwrap();
}
