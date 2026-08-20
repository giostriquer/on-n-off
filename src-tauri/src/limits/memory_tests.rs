use super::*;
use crate::dto::LimitWindowKind;
use crate::paths::scratch_dir;
use json::window;

fn account(id: &str, label: &str) -> LimitsAccountDto {
    LimitsAccountDto {
        id: id.to_string(),
        label: Some(label.to_string()),
    }
}

fn ok_snapshot(provider: AgentId, id: &str, label: &str, used: f64) -> ProviderLimitsDto {
    let mut dto = finish(
        provider,
        LimitsStatus::Ok,
        None,
        Parsed {
            account: Some(account(id, label)),
            plan: Some("pro".to_string()),
            windows: vec![window(
                "primary",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                used,
                None,
            )],
            credits: None,
        },
    );
    let observed_at = format!("2026-08-17T{:02}:00:00.000Z", used as u32 % 24);
    for window in &mut dto.windows {
        window.observed_at.clone_from(&observed_at);
    }
    dto
}

#[test]
fn current_read_is_remembered_once_ahead_of_other_accounts() {
    let home = scratch_dir("limits-memory");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&ok_snapshot(AgentId::Codex, "acct-a", "a@x", 5.0))
        .unwrap();
    store
        .save(&ok_snapshot(AgentId::Codex, "acct-b", "b@x", 9.0))
        .unwrap();

    let mut current = ok_snapshot(AgentId::Codex, "acct-a", "a@x", 50.0);
    current.windows[0].observed_at = "2026-08-17T23:00:00.000Z".to_string();
    let listed = aggregate_accounts(&store, current, None);
    let summary: Vec<(&str, bool, f64)> = listed
        .iter()
        .map(|dto| {
            (
                dto.account.as_ref().unwrap().id.as_str(),
                dto.current_account,
                dto.windows[0].used_percent,
            )
        })
        .collect();
    assert_eq!(summary, [("acct-a", true, 50.0), ("acct-b", false, 9.0)]);
    let remembered_a = store
        .load(AgentId::Codex)
        .into_iter()
        .find(|dto| dto.account.as_ref().unwrap().id == "acct-a")
        .unwrap();
    assert_eq!(remembered_a.windows[0].used_percent, 50.0);
}

#[test]
fn failed_anonymous_read_hides_no_remembered_account_and_saves_nothing() {
    let home = scratch_dir("limits-memory");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&ok_snapshot(AgentId::Claude, "uuid-a", "a@x", 5.0))
        .unwrap();
    let signed_out = finish(
        AgentId::Claude,
        LimitsStatus::SignedOut,
        Some("Sign in".to_string()),
        Parsed::default(),
    );

    let listed = aggregate_accounts(&store, signed_out, None);

    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].status, LimitsStatus::SignedOut);
    assert!(listed[0].current_account);
    assert_eq!(listed[1].account.as_ref().unwrap().id, "uuid-a");
    assert!(!listed[1].current_account);
    assert_eq!(store.load(AgentId::Claude).len(), 1);
}

#[test]
fn successful_read_without_an_account_is_not_remembered() {
    let home = scratch_dir("limits-memory");
    let store = SnapshotStore::for_home(&home);
    let anonymous = finish(
        AgentId::Codex,
        LimitsStatus::Ok,
        None,
        Parsed {
            plan: Some("max".to_string()),
            windows: vec![window(
                "w",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                1.0,
                None,
            )],
            ..Parsed::default()
        },
    );

    let listed = aggregate_accounts(&store, anonymous, None);

    assert_eq!(listed.len(), 1);
    assert!(store.load(AgentId::Codex).is_empty());
}

#[test]
fn failed_read_keeps_the_signed_in_accounts_last_numbers_in_one_card() {
    let home = scratch_dir("limits-memory");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&ok_snapshot(AgentId::Claude, "uuid-a", "a@x", 39.0))
        .unwrap();
    store
        .save(&ok_snapshot(AgentId::Claude, "uuid-b", "b@x", 5.0))
        .unwrap();
    let stalled = finish(
        AgentId::Claude,
        LimitsStatus::Unauthenticated,
        Some("Access token expired".to_string()),
        Parsed {
            account: Some(account("uuid-a", "a@x")),
            ..Parsed::default()
        },
    );

    let listed = aggregate_accounts(&store, stalled, None);

    assert_eq!(listed.len(), 2, "no blank card above the account's numbers");
    assert_eq!(listed[0].status, LimitsStatus::Unauthenticated);
    assert_eq!(listed[0].message.as_deref(), Some("Access token expired"));
    assert!(
        listed[0].current_account,
        "it is still the signed-in account"
    );
    assert_eq!(listed[0].windows[0].used_percent, 39.0);
    assert_eq!(listed[0].plan.as_deref(), Some("pro"));
    assert_eq!(listed[0].windows[0].observed_at, "2026-08-17T15:00:00.000Z");
    assert_eq!(listed[1].account.as_ref().unwrap().id, "uuid-b");
    assert!(!listed[1].current_account);
}

#[test]
fn successful_endpoint_and_local_windows_merge_and_persist_per_observation_time() {
    let home = scratch_dir("limits-memory-successful-local-merge");
    let store = SnapshotStore::for_home(&home);
    let mut current = ok_snapshot(AgentId::Claude, "uuid-a", "a@x", 50.0);
    current.windows[0].id = "weekly_all".to_string();
    current.windows[0].observed_at = "2026-08-20T12:00:00.000Z".to_string();
    let supplemental = ObservedWindowSet::local(
        chrono::DateTime::parse_from_rfc3339("2026-08-20T13:00:00.000Z")
            .unwrap()
            .with_timezone(&Utc),
        vec![window(
            "session",
            "5 hour · all models",
            LimitWindowKind::Session,
            75.0,
            None,
        )],
    );

    let listed = aggregate_accounts(&store, current, Some(supplemental));

    assert_eq!(listed[0].windows.len(), 2);
    assert_eq!(listed[0].windows[1].id, "session");
    assert_eq!(listed[0].windows[1].used_percent, 75.0);
    let persisted = store.load(AgentId::Claude);
    assert_eq!(persisted[0].windows.len(), 2);
    assert_eq!(persisted[0].windows[1].id, "session");
}
