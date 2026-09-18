use super::*;
use crate::dto::{AgentId, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto};
use crate::paths::scratch_dir;
use std::fs;

impl SnapshotStore {
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

fn snapshot(provider: AgentId, id: &str, label: &str, observed_at: &str) -> ProviderLimitsDto {
    let mut dto = ProviderLimitsDto {
        provider,
        status: LimitsStatus::Ok,
        message: None,
        account: Some(LimitsAccountDto {
            legacy_id: None,
            id: id.to_string(),
            label: Some(label.to_string()),
        }),
        current_account: true,
        plan: Some("pro".to_string()),
        windows: vec![super::super::json::window(
            "primary",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            42.0,
            Some("2026-08-24T23:34:33+00:00".to_string()),
        )],
        credits: None,
        reset_credits: None,
        reset_offer: None,
    };
    for window in &mut dto.windows {
        window.observed_at = observed_at.to_string();
    }
    dto
}

#[test]
fn legacy_removal_rechecks_the_stored_email_after_another_account_replaces_history() {
    let home = scratch_dir("limits-forget-matching");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&snapshot(
            AgentId::Codex,
            "team",
            "first@example.com",
            "2026-09-13T12:00:00Z",
        ))
        .unwrap();
    // The UI confirmed the first account, but another writer replaced its workspace-keyed row.
    store
        .save(&snapshot(
            AgentId::Codex,
            "team",
            "second@example.com",
            "2026-09-13T13:00:00Z",
        ))
        .unwrap();
    assert!(store
        .forget_matching_email(AgentId::Codex, "team", "first@example.com")
        .is_err());
    assert_eq!(
        store.load(AgentId::Codex)[0]
            .account
            .as_ref()
            .unwrap()
            .label
            .as_deref(),
        Some("second@example.com")
    );
    store
        .forget_matching_email(AgentId::Codex, "team", " Second@Example.com ")
        .unwrap();
    assert!(store.load(AgentId::Codex).is_empty());
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn saved_snapshots_load_back_per_provider_newest_first_and_not_current() {
    let home = scratch_dir("limits-snap");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&snapshot(
            AgentId::Codex,
            "acct-old",
            "old@x",
            "2026-08-17T10:30:00.000+01:00",
        ))
        .unwrap();
    store
        .save(&snapshot(
            AgentId::Codex,
            "acct-new",
            "new@x",
            "2026-08-17T10:00:00.000Z",
        ))
        .unwrap();
    store
        .save(&snapshot(
            AgentId::Claude,
            "uuid-1",
            "me@x",
            "2026-08-17T11:00:00.000Z",
        ))
        .unwrap();

    let codex = store.load(AgentId::Codex);
    let ids: Vec<&str> = codex
        .iter()
        .map(|dto| dto.account.as_ref().unwrap().id.as_str())
        .collect();
    assert_eq!(ids, ["acct-new", "acct-old"]);
    assert!(codex
        .iter()
        .all(|dto| !dto.current_account && dto.status == LimitsStatus::Ok));
    assert_eq!(codex[0].windows[0].used_percent, 42.0);
    assert_eq!(codex[0].plan.as_deref(), Some("pro"));

    let claude = store.load(AgentId::Claude);
    assert_eq!(claude.len(), 1);
    assert_eq!(
        claude[0].account.as_ref().unwrap().label.as_deref(),
        Some("me@x")
    );
}

#[test]
fn saving_the_same_account_again_replaces_its_snapshot() {
    let home = scratch_dir("limits-snap");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&snapshot(
            AgentId::Codex,
            "acct-1",
            "a@x",
            "2026-08-16T10:00:00.000Z",
        ))
        .unwrap();
    let mut newer = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
    newer.windows[0].used_percent = 7.0;
    store.save(&newer).unwrap();
    let loaded = store.load(AgentId::Codex);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].windows[0].used_percent, 7.0);
    assert_eq!(loaded[0].windows[0].observed_at, "2026-08-17T10:00:00.000Z");
}

#[test]
fn an_older_save_cannot_replace_a_newer_snapshot_for_the_same_account() {
    let home = scratch_dir("limits-snap-freshness");
    let store = SnapshotStore::for_home(&home);
    let mut newer = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
    newer.windows[0].used_percent = 7.0;
    store.save(&newer).unwrap();
    let mut older = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-16T10:00:00.000Z");
    older.windows[0].used_percent = 99.0;

    store.save(&older).unwrap();

    let loaded = store.load(AgentId::Codex);
    assert_eq!(loaded[0].windows[0].used_percent, 7.0);
    assert_eq!(loaded[0].windows[0].observed_at, "2026-08-17T10:00:00.000Z");
}

#[test]
fn a_newer_successful_credits_only_snapshot_removes_old_quota_windows() {
    let home = scratch_dir("limits-snap-credits-freshness");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&snapshot(
            AgentId::Codex,
            "acct-1",
            "a@x",
            "2000-01-01T00:00:00.000Z",
        ))
        .unwrap();
    let credits_only = ProviderLimitsDto {
        provider: AgentId::Codex,
        status: LimitsStatus::Ok,
        message: None,
        account: Some(LimitsAccountDto {
            legacy_id: None,
            id: "acct-1".to_string(),
            label: Some("a@x".to_string()),
        }),
        current_account: true,
        plan: Some("pro".to_string()),
        windows: Vec::new(),
        credits: Some(LimitsCreditsDto {
            balance: "3".to_string(),
            unlimited: false,
        }),
        reset_credits: None,
        reset_offer: None,
    };

    store.save(&credits_only).unwrap();

    let loaded = store.load(AgentId::Codex);
    assert_eq!(loaded.len(), 1);
    assert!(loaded[0].windows.is_empty());
    assert_eq!(loaded[0].credits, credits_only.credits);
}

#[test]
fn snapshots_require_an_account_and_observations_but_not_an_ok_endpoint_status() {
    let home = scratch_dir("limits-snap");
    let store = SnapshotStore::for_home(&home);
    let mut anonymous = snapshot(AgentId::Codex, "x", "x", "2026-08-17T10:00:00.000Z");
    anonymous.account = None;
    assert!(store.save(&anonymous).is_err());
    let mut empty = snapshot(
        AgentId::Codex,
        "acct-empty",
        "empty@x",
        "2026-08-17T10:00:00.000Z",
    );
    empty.windows.clear();
    assert!(store.save(&empty).is_err());
    let mut failed = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
    failed.status = LimitsStatus::Failed;
    assert!(store.save(&failed).is_ok());
    let loaded = store.load(AgentId::Codex);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].account.as_ref().unwrap().id, "acct-1");
    assert_eq!(loaded[0].windows[0].used_percent, 42.0);
}

#[test]
fn forget_removes_one_account_and_load_skips_unreadable_files() {
    let home = scratch_dir("limits-snap");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&snapshot(
            AgentId::Codex,
            "acct-1",
            "a@x",
            "2026-08-16T10:00:00.000Z",
        ))
        .unwrap();
    store
        .save(&snapshot(
            AgentId::Codex,
            "acct-2",
            "b@x",
            "2026-08-17T10:00:00.000Z",
        ))
        .unwrap();
    fs::write(store.dir().join("codex-broken.json"), "{nope").unwrap();
    fs::write(store.dir().join("notes.txt"), "ignore me").unwrap();

    store.forget(AgentId::Codex, "acct-2").unwrap();
    let loaded = store.load(AgentId::Codex);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].account.as_ref().unwrap().id, "acct-1");
    // Forgetting something unknown is not an error.
    store.forget(AgentId::Codex, "acct-2").unwrap();
}

#[test]
fn ids_that_sanitise_alike_stay_distinct_files() {
    let home = scratch_dir("limits-snap");
    let store = SnapshotStore::for_home(&home);
    let long_a = format!("{}A", "x".repeat(60));
    let long_b = format!("{}B", "x".repeat(60));
    for id in ["a/b", "a_b", long_a.as_str(), long_b.as_str()] {
        store
            .save(&snapshot(
                AgentId::Codex,
                id,
                "e@x",
                "2026-08-17T10:00:00.000Z",
            ))
            .unwrap();
    }
    assert_eq!(store.load(AgentId::Codex).len(), 4);
    store.forget(AgentId::Codex, "a/b").unwrap();
    let ids: Vec<String> = store
        .load(AgentId::Codex)
        .into_iter()
        .map(|dto| dto.account.unwrap().id)
        .collect();
    assert!(!ids.iter().any(|id| id == "a/b"));
    assert!(ids.iter().any(|id| id == "a_b"));
    assert_eq!(ids.len(), 3);
}

#[test]
fn obsolete_v1_snapshots_are_ignored_without_rewriting_them() {
    let home = scratch_dir("limits-snap");
    let store = SnapshotStore::for_home(&home);
    fs::create_dir_all(store.dir()).unwrap();
    let path = store.dir().join("claude-uuid1-00000000.json");
    fs::write(
        &path,
        r#"{"provider":"claude","status":"ok","account":{"id":"uuid-1"},"live":true,"plan":"max","windows":[{"id":"weekly_all","label":"Weekly · all models","kind":"weekly","usedPercent":42,"resetsAt":"2026-08-17T10:00:00Z"}],"fetchedAt":"2026-08-10T10:00:00.000Z"}"#,
    )
    .unwrap();
    let loaded = store.load(AgentId::Claude);
    assert!(loaded.is_empty());
    let unchanged = fs::read_to_string(path).unwrap();
    assert!(unchanged.contains("\"fetchedAt\""));
    assert!(!unchanged.contains("\"schemaVersion\""));
}

#[test]
fn account_ids_are_made_safe_for_file_names() {
    let home = scratch_dir("limits-snap");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&snapshot(
            AgentId::Claude,
            "../evil/../id with spaces",
            "e@x",
            "2026-08-17T10:00:00.000Z",
        ))
        .unwrap();
    let files: Vec<String> = fs::read_dir(store.dir())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(files.len(), 1);
    assert!(files[0].starts_with("claude-"), "{files:?}");
    assert!(
        !files[0].contains('/') && !files[0].contains(' ') && !files[0].contains(".."),
        "{files:?}"
    );
    assert_eq!(
        store.load(AgentId::Claude)[0].account.as_ref().unwrap().id,
        "../evil/../id with spaces"
    );
}

#[test]
fn simultaneous_store_instances_cannot_replace_a_newer_observation_with_an_older_one() {
    use std::sync::Barrier;
    for _ in 0..8 {
        let home = tempfile::tempdir().unwrap();
        let start = Barrier::new(16);
        std::thread::scope(|threads| {
            for second in 0..16 {
                let home = home.path();
                let start = &start;
                threads.spawn(move || {
                    let mut dto = snapshot(
                        AgentId::Codex,
                        "profile:shared",
                        "me@x",
                        &format!("2026-09-13T12:00:{second:02}Z"),
                    );
                    dto.windows[0].used_percent = f64::from(second);
                    start.wait();
                    SnapshotStore::for_home(home).save(&dto).unwrap();
                });
            }
        });
        let loaded = SnapshotStore::for_home(home.path()).load(AgentId::Codex);
        assert_eq!(loaded.len(), 1);
        assert_eq!(
            loaded[0].windows[0].used_percent, 15.0,
            "concurrent publication lost the newest usage"
        );
    }
}

#[test]
fn remembered_reset_credits_survive_a_reload_and_older_snapshots_load_without_them() {
    let home = scratch_dir("limits-snap-reset-credits");
    let store = SnapshotStore::for_home(&home);
    let mut dto = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
    dto.reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 1,
        next_expires_at: Some("2026-09-01T12:00:00+00:00".to_string()),
    });
    store.save(&dto).unwrap();
    assert_eq!(
        store.load(AgentId::Codex)[0].reset_credits,
        dto.reset_credits
    );

    // A snapshot written before on-n-off knew about reset credits has no such key.
    let path = fs::read_dir(store.dir())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
        .unwrap();
    let mut stored: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(stored
        .as_object_mut()
        .unwrap()
        .remove("resetCredits")
        .is_some());
    fs::write(&path, stored.to_string()).unwrap();

    let loaded = store.load(AgentId::Codex);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].reset_credits, None);
}

#[test]
fn a_successful_read_with_only_banked_resets_is_remembered_and_dated() {
    let home = scratch_dir("limits-snap-reset-credits-only");
    let store = SnapshotStore::for_home(&home);
    let mut dto = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
    dto.windows.clear();
    dto.reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 1,
        next_expires_at: None,
    });

    store.save(&dto).unwrap();

    let loaded = store.load(AgentId::Codex);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].reset_credits, dto.reset_credits);
}

#[test]
fn quota_windows_credits_and_banked_resets_each_count_as_an_observation() {
    let mut dto = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
    assert!(dto.has_observations());
    dto.windows.clear();
    assert!(!dto.has_observations());
    dto.credits = Some(LimitsCreditsDto {
        balance: "0".to_string(),
        unlimited: false,
    });
    assert!(dto.has_observations());
    dto.credits = None;
    // Every current Codex read reports a count, usually 0; on its own that observed nothing.
    dto.reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 0,
        next_expires_at: None,
    });
    assert!(!dto.has_observations());
    dto.reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 1,
        next_expires_at: None,
    });
    assert!(dto.has_observations());
    dto.reset_credits = None;
    // An offer is what the provider is selling right now, not something observed about the account.
    dto.reset_offer = Some(crate::dto::LimitsResetOfferDto { price: None });
    assert!(!dto.has_observations());
}

#[test]
fn a_paid_reset_offer_is_never_written_to_a_snapshot_or_read_back_from_one() {
    let home = scratch_dir("limits-snap-reset-offer");
    let store = SnapshotStore::for_home(&home);
    let mut dto = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
    dto.reset_offer = Some(crate::dto::LimitsResetOfferDto {
        price: Some(crate::dto::LimitsPriceDto {
            amount_minor_units: 800,
            currency: "USD".to_string(),
        }),
    });
    store.save(&dto).unwrap();

    // Not in the file, so no later version can start reading a price the provider has withdrawn.
    let written: Vec<String> = fs::read_dir(store.dir())
        .unwrap()
        .filter_map(|entry| fs::read_to_string(entry.ok()?.path()).ok())
        .collect();
    assert!(!written.is_empty());
    for file in &written {
        assert!(!file.contains("resetOffer"), "{file}");
        assert!(!file.contains("800"), "{file}");
    }
    assert!(store
        .load(AgentId::Codex)
        .iter()
        .all(|remembered| remembered.reset_offer.is_none()));
}

#[test]
fn a_windowless_read_that_reports_no_banked_resets_keeps_the_remembered_windows() {
    let home = scratch_dir("limits-snap-reset-credits-zero");
    let store = SnapshotStore::for_home(&home);
    let remembered = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
    store.save(&remembered).unwrap();
    // The shape a current Codex CLI returns when it reports no windows: the count is still there.
    let mut windowless = remembered.clone();
    windowless.windows.clear();
    windowless.reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 0,
        next_expires_at: None,
    });

    assert!(store.save(&windowless).is_err());

    assert_eq!(store.load(AgentId::Codex)[0].windows, remembered.windows);
}

#[test]
fn saving_after_a_merge_rewrites_only_the_accounts_the_merge_changed() {
    let home = scratch_dir("limits-snap-save-changed");
    let store = SnapshotStore::for_home(&home);
    let mut credits_only = snapshot(
        AgentId::Codex,
        "acct-credits",
        "c@x",
        "2026-08-17T10:00:00.000Z",
    );
    credits_only.windows.clear();
    credits_only.credits = Some(LimitsCreditsDto {
        balance: "3".to_string(),
        unlimited: false,
    });
    let windowed = snapshot(
        AgentId::Codex,
        "acct-windows",
        "w@x",
        "2026-08-17T10:00:00.000Z",
    );
    store.save(&credits_only).unwrap();
    store.save(&windowed).unwrap();
    let stored = |id: &str| {
        fs::read_dir(store.dir())
            .unwrap()
            .flatten()
            .map(|entry| fs::read_to_string(entry.path()).unwrap())
            .find(|raw| raw.contains(id))
            .unwrap()
    };
    let credits_before = stored("acct-credits");
    let before = vec![credits_only.clone(), windowed.clone()];
    let mut after = before.clone();
    after[1].windows[0].used_percent = 60.0;
    after[1].windows[0].observed_at = "2026-08-17T11:00:00.000Z".to_string();

    store.save_changed(&before, &after);

    // An untouched account keeps its own observation date instead of being re-dated now.
    assert_eq!(stored("acct-credits"), credits_before);
    assert!(stored("acct-windows").contains("\"usedPercent\":60.0"));
}
