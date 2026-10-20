use super::account;
use crate::dto::LimitWindowKind;
use crate::limits::window::fixture;
use crate::limits::*;
use crate::paths::scratch_dir;

fn ok_snapshot(provider: AgentId, id: &str, label: &str, used: f64) -> ProviderLimitsDto {
    let mut dto = finish(
        provider,
        LimitsStatus::Ok,
        None,
        Parsed {
            account: Some(account(id, label)),
            reading: Reading {
                plan: Some("pro".to_string()),
                windows: vec![fixture(
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
fn a_signed_out_read_names_nobody_and_leaves_the_remembered_account_its_own_card() {
    let home = scratch_dir("limits-memory");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&ok_snapshot(AgentId::Claude, "uuid-a", "a@x", 5.0))
        .unwrap();
    let (signed_out, _) = outcome::signed_in(
        AgentId::Claude,
        outcome::Read {
            account: Some(account("uuid-a", "a@x")),
            answer: Err(ReadFailure::SignedOut),
        },
    );

    let listed = aggregate_accounts(&store, signed_out);

    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].status, LimitsStatus::SignedOut);
    assert!(listed[0].current_account);
    assert!(listed[0].account.is_none());
    assert_eq!(listed[1].account.as_ref().unwrap().id, "uuid-a");
    assert!(!listed[1].current_account);
    assert_eq!(store.load(AgentId::Claude).len(), 1);
}

#[test]
fn a_failed_read_of_a_known_account_shows_its_remembered_reading_on_the_one_current_card() {
    for provider in crate::provider_catalogue::WITH_LIMITS {
        let home = scratch_dir("limits-memory-failed-known");
        let store = SnapshotStore::for_home(&home);
        store
            .save(&ok_snapshot(provider, "acct-a", "a@x", 5.0))
            .unwrap();
        let (failed, _) = outcome::signed_in(
            provider,
            outcome::Read {
                account: Some(LimitsAccountDto {
                    legacy_id: None,
                    id: "acct-a".into(),
                    label: None,
                }),
                answer: Err(ReadFailure::Unavailable("The source said why.".into())),
            },
        );

        let listed = aggregate_accounts(&store, failed);

        assert_eq!(listed.len(), 1, "{provider:?}");
        assert!(listed[0].current_account, "{provider:?}");
        assert_eq!(listed[0].status, LimitsStatus::Failed, "{provider:?}");
        assert_eq!(
            listed[0].reading.windows[0].used_percent, 5.0,
            "{provider:?}"
        );
        assert_eq!(
            listed[0].account.as_ref().unwrap().label.as_deref(),
            Some("a@x"),
            "{provider:?}: the card lost the remembered name"
        );
        assert_eq!(
            store.load(provider)[0]
                .account
                .as_ref()
                .unwrap()
                .label
                .as_deref(),
            Some("a@x"),
            "{provider:?}: the file lost the remembered name"
        );
    }
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
                windows: vec![fixture(
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

fn folded(cards: &[ProviderLimitsDto]) -> Vec<ProviderLimitsDto> {
    column(
        cards,
        &crate::accounts::Roster::default(),
        &crate::accounts::SignInDeadlines::new(),
        0,
    )
    .into_iter()
    .map(|entry| entry.card)
    .collect()
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
        let listed = folded(&aggregate_accounts(&store, current));
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
        let reloaded = folded(&SnapshotStore::for_home(&home).load(provider));
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
    let listed = folded(&aggregate_accounts(&store, current));
    assert_eq!(listed.len(), 4);
    assert_eq!(
        folded(&SnapshotStore::for_home(&home).load(AgentId::Codex)).len(),
        4
    );
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
    let listed = folded(&aggregate_accounts(&store, current));
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
    let listed = folded(&aggregate_accounts(&store, current));
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

#[test]
fn a_read_that_names_its_account_keeps_that_name_over_the_remembered_one() {
    for remembered_name in [Some("old@x"), None] {
        let home = scratch_dir("limits-memory-new-name");
        let store = SnapshotStore::for_home(&home);
        let mut stored = ok_snapshot(AgentId::Codex, "acct-a", "old@x", 5.0);
        stored.account.as_mut().unwrap().label = remembered_name.map(str::to_string);
        store.save(&stored).unwrap();

        let remembered = store.remember(ok_snapshot(AgentId::Codex, "acct-a", "new@x", 9.0));

        assert_eq!(
            remembered.card.account.unwrap().label.as_deref(),
            Some("new@x"),
            "{remembered_name:?}"
        );
        assert_eq!(
            store.load(AgentId::Codex)[0]
                .account
                .as_ref()
                .unwrap()
                .label
                .as_deref(),
            Some("new@x"),
            "{remembered_name:?}"
        );
    }
}
