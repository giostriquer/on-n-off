use crate::dto::{LimitWindowKind, LimitsAccountDto};
use crate::limits::json::window;
use crate::limits::*;
use crate::paths::scratch_dir;

fn account(id: &str, label: &str) -> LimitsAccountDto {
    LimitsAccountDto {
        legacy_id: None,
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
            reading: Reading {
                plan: Some("pro".to_string()),
                windows: vec![window(
                    "primary",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    used,
                    None,
                )],
                ..Reading::default()
            },
        },
    );
    let observed_at = format!("2026-08-17T{:02}:00:00.000Z", used as u32 % 24);
    for window in &mut dto.reading.windows {
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
    current.reading.windows[0].observed_at = "2026-08-17T23:00:00.000Z".to_string();
    let listed = aggregate_accounts(&store, current);
    let summary: Vec<(&str, bool, f64)> = listed
        .iter()
        .map(|dto| {
            (
                dto.account.as_ref().unwrap().id.as_str(),
                dto.current_account,
                dto.reading.windows[0].used_percent,
            )
        })
        .collect();
    assert_eq!(summary, [("acct-a", true, 50.0), ("acct-b", false, 9.0)]);
    let remembered_a = store
        .load(AgentId::Codex)
        .into_iter()
        .find(|dto| dto.account.as_ref().unwrap().id == "acct-a")
        .unwrap();
    assert_eq!(remembered_a.reading.windows[0].used_percent, 50.0);
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

    let listed = aggregate_accounts(&store, signed_out);

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
            account: None,
            reading: Reading {
                plan: Some("max".to_string()),
                windows: vec![window(
                    "w",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    1.0,
                    None,
                )],
                ..Reading::default()
            },
        },
    );

    let listed = aggregate_accounts(&store, anonymous);

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

    let listed = aggregate_accounts(&store, stalled);

    assert_eq!(listed.len(), 2, "no blank card above the account's numbers");
    assert_eq!(listed[0].status, LimitsStatus::Unauthenticated);
    assert_eq!(listed[0].message.as_deref(), Some("Access token expired"));
    assert!(
        listed[0].current_account,
        "it is still the signed-in account"
    );
    assert_eq!(listed[0].reading.windows[0].used_percent, 39.0);
    assert_eq!(listed[0].reading.plan.as_deref(), Some("pro"));
    assert_eq!(
        listed[0].reading.windows[0].observed_at,
        "2026-08-17T15:00:00.000Z"
    );
    assert_eq!(listed[1].account.as_ref().unwrap().id, "uuid-b");
    assert!(!listed[1].current_account);
}

fn scoped_snapshot(
    provider: AgentId,
    id: &str,
    legacy: &str,
    email: &str,
    used: f64,
) -> ProviderLimitsDto {
    let mut value = serde_json::to_value(ok_snapshot(provider, id, email, used)).unwrap();
    value["account"]["legacyId"] = serde_json::json!(legacy);
    serde_json::from_value(value).unwrap()
}

#[test]
fn upgraded_identity_shows_one_card_and_stays_deduplicated_after_reload() {
    for provider in [AgentId::Claude, AgentId::Codex] {
        let home = scratch_dir("limits-identity-upgrade");
        let store = SnapshotStore::for_home(&home);
        store
            .save(&ok_snapshot(provider, "legacy-a", "a@x", 90.0))
            .unwrap();
        store
            .save(&ok_snapshot(
                provider,
                "legacy-icloud",
                "a@icloud.com",
                70.0,
            ))
            .unwrap();
        let current = scoped_snapshot(provider, "profile:scoped-a", "legacy-a", "a@x", 5.0);
        let listed = aggregate_accounts(&store, current);
        assert_eq!(
            listed.len(),
            2,
            "one active card plus the other remembered account"
        );
        assert_eq!(
            listed[0].reading.windows[0].used_percent, 5.0,
            "legacy windows are not transferred"
        );
        assert_eq!(
            listed[1].account.as_ref().unwrap().label.as_deref(),
            Some("a@icloud.com")
        );
        let reloaded = SnapshotStore::for_home(&home).load(provider);
        assert_eq!(
            reloaded.len(),
            2,
            "old identity stays superseded after restarting"
        );
        assert!(reloaded
            .iter()
            .all(|d| d.account.as_ref().unwrap().id != "legacy-a"));
        store.forget(provider, "profile:scoped-a").unwrap();
        let remaining = SnapshotStore::for_home(&home).load(provider);
        assert_eq!(
            remaining.len(),
            1,
            "Forget must not resurrect the superseded card"
        );
        assert_eq!(remaining[0].account.as_ref().unwrap().id, "legacy-icloud");
    }
}

#[test]
fn identity_upgrade_does_not_merge_accounts_by_email_or_workspace_alone() {
    let home = scratch_dir("limits-identity-upgrade-isolation");
    let store = SnapshotStore::for_home(&home);
    // A different user in the same workspace, and the same email in a different workspace.
    store
        .save(&ok_snapshot(AgentId::Codex, "workspace-a", "other@x", 70.0))
        .unwrap();
    store
        .save(&ok_snapshot(AgentId::Codex, "workspace-b", "a@x", 80.0))
        .unwrap();
    store
        .save(&scoped_snapshot(
            AgentId::Codex,
            "profile:other-workspace",
            "workspace-c",
            "a@x",
            60.0,
        ))
        .unwrap();
    let current = scoped_snapshot(AgentId::Codex, "profile:current", "workspace-a", "a@x", 5.0);
    let listed = aggregate_accounts(&store, current);
    assert_eq!(listed.len(), 4);
    assert_eq!(SnapshotStore::for_home(&home).load(AgentId::Codex).len(), 4);
}

#[test]
fn failed_upgrade_keeps_unscoped_history_without_attributing_it_to_the_new_identity() {
    let home = scratch_dir("limits-identity-upgrade-failed");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&ok_snapshot(AgentId::Codex, "workspace-a", "a@x", 70.0))
        .unwrap();
    let mut current = scoped_snapshot(AgentId::Codex, "profile:current", "workspace-a", "a@x", 5.0);
    current.status = LimitsStatus::Failed;
    current.reading.windows.clear();
    let listed = aggregate_accounts(&store, current);
    assert_eq!(listed.len(), 2);
    assert!(listed[0].reading.windows.is_empty());
    assert_eq!(listed[1].reading.windows[0].used_percent, 70.0);
}

#[test]
fn saved_scoped_history_never_hides_a_current_unscoped_error() {
    let home = scratch_dir("limits-identity-incomplete-current");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&scoped_snapshot(
            AgentId::Claude,
            "profile:known",
            "user-a",
            "a@x",
            70.0,
        ))
        .unwrap();
    let mut current = ok_snapshot(AgentId::Claude, "user-a", "a@x", 0.0);
    current.status = LimitsStatus::Failed;
    current.message = Some("Current account could not be verified".into());
    current.reading.windows.clear();
    let listed = aggregate_accounts(&store, current);
    assert_eq!(
        listed.len(),
        2,
        "current error must remain alongside remembered history"
    );
    assert!(listed[0].current_account);
    assert_eq!(listed[0].status, LimitsStatus::Failed);
    assert_eq!(
        listed[0].message.as_deref(),
        Some("Current account could not be verified")
    );
    assert!(listed[0].reading.windows.is_empty());
}
