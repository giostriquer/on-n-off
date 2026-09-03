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
    };
    for window in &mut dto.windows {
        window.observed_at = observed_at.to_string();
    }
    dto
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
