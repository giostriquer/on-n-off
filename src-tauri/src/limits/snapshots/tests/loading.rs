//! What loading gives back: files written before a figure existed, files from an obsolete
//! schema, banked-reset counts whose soonest expiry has passed, and Codex files that still hold
//! hidden windows.

use super::*;

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
fn remembered_reset_credits_survive_a_reload_and_older_snapshots_load_without_them() {
    let home = scratch_dir("limits-snap-reset-credits");
    let store = SnapshotStore::for_home(&home);
    let mut dto = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
    dto.reading.reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 1,
        next_expires_at: Some("2100-09-01T12:00:00+00:00".to_string()),
    });
    store.save(&dto).unwrap();
    assert_eq!(
        store.load(AgentId::Codex)[0].reading.reset_credits,
        dto.reading.reset_credits
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
    assert_eq!(loaded[0].reading.reset_credits, None);
}

/// A remembered count stops at its soonest known expiry: by then at least one reset has lapsed and
/// what is left is not known until a read answers again. The windows beside it stay remembered, and
/// a snapshot left with nothing observed is not loaded, as none would be saved.
#[test]
fn a_remembered_banked_reset_count_past_its_soonest_expiry_loads_as_unknown() {
    let home = scratch_dir("limits-snap-reset-credits-lapsed");
    let store = SnapshotStore::for_home(&home);
    let save = |id: &str, windows: bool, next_expires_at: Option<&str>| {
        let mut dto = snapshot(AgentId::Codex, id, "a@x", "2026-08-17T10:00:00.000Z");
        if !windows {
            dto.reading.windows.clear();
        }
        dto.reading.reset_credits = Some(crate::dto::LimitsResetCreditsDto {
            available_count: 2,
            next_expires_at: next_expires_at.map(str::to_owned),
        });
        store.save(&dto).unwrap();
        dto
    };
    let lapsed = save("lapsed", true, Some("2020-01-01T00:00:00+00:00"));
    let ahead = save("ahead", true, Some("2100-01-01T00:00:00+00:00"));
    let undated = save("undated", true, None);
    save(
        "lapsed-count-only",
        false,
        Some("2020-01-01T00:00:00+00:00"),
    );

    let loaded = store.load(AgentId::Codex);
    let find = |id: &str| {
        loaded
            .iter()
            .find(|dto| dto.account.as_ref().is_some_and(|account| account.id == id))
    };
    assert_eq!(find("lapsed").unwrap().reading.reset_credits, None);
    assert_eq!(
        find("lapsed").unwrap().reading.windows,
        lapsed.reading.windows
    );
    assert_eq!(
        find("ahead").unwrap().reading.reset_credits,
        ahead.reading.reset_credits
    );
    assert_eq!(
        find("undated").unwrap().reading.reset_credits,
        undated.reading.reset_credits
    );
    assert!(find("lapsed-count-only").is_none());
    assert_eq!(loaded.len(), 3);
}

/// A saved account whose only observation was a count that has since lapsed stops replacing the
/// history it superseded, since it no longer holds an observation. Forgetting it still takes that
/// history with it, as it did while the count stood.
#[test]
fn forgetting_an_account_whose_count_lapsed_still_removes_the_history_it_replaced() {
    let home = scratch_dir("limits-snap-lapsed-forget");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&snapshot(
            AgentId::Codex,
            "team",
            "a@x",
            "2026-08-17T10:00:00.000Z",
        ))
        .unwrap();
    let mut scoped = snapshot(
        AgentId::Codex,
        "profile:abc",
        "a@x",
        "2026-08-17T11:00:00.000Z",
    );
    scoped.account.as_mut().unwrap().legacy_id = Some("team".to_string());
    scoped.reading.windows.clear();
    scoped.reading.reset_credits = Some(crate::dto::LimitsResetCreditsDto {
        available_count: 1,
        next_expires_at: Some("2020-01-01T00:00:00+00:00".to_string()),
    });
    store.save(&scoped).unwrap();
    let ids = |store: &SnapshotStore| {
        store
            .load(AgentId::Codex)
            .into_iter()
            .map(|dto| dto.account.unwrap().id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&store), ["team"]);

    store.forget(AgentId::Codex, "profile:abc").unwrap();
    assert!(ids(&store).is_empty());
}

/// The subscription status is remembered with the account, and a snapshot written before it existed
/// still loads, without one.
#[test]
fn a_remembered_snapshot_keeps_the_subscription_status() {
    let home = scratch_dir("limits-snap-subscription-status");
    let store = SnapshotStore::for_home(&home);
    let mut dto = snapshot(AgentId::Claude, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
    dto.reading.subscription_status = Some("past_due".to_string());
    store.save(&dto).unwrap();

    assert_eq!(
        store.load(AgentId::Claude)[0]
            .reading
            .subscription_status
            .as_deref(),
        Some("past_due")
    );

    let old = serde_json::json!({
        "schemaVersion": 2, "provider": "claude",
        "account": {"id": "acct-2", "label": "b@x"},
        "windows": [{"id": "seven_day", "label": "Weekly", "kind": "weekly", "usedPercent": 10.0,
                     "observedAt": "2026-08-17T10:00:00.000Z"}]
    });
    std::fs::write(
        home.join(".on-n-off/limits/claude-acct_2-00000000.json"),
        old.to_string(),
    )
    .unwrap();
    let loaded = store.load(AgentId::Claude);
    let older = loaded
        .iter()
        .find(|dto| dto.account.as_ref().unwrap().id == "acct-2")
        .expect("a snapshot written before the field loads");
    assert_eq!(older.reading.subscription_status, None);
    let _ = std::fs::remove_dir_all(&home);
}

/// The term is remembered with the card, and a snapshot written before it existed loads without one.
#[test]
fn a_remembered_term_loads_back() {
    let home = scratch_dir("limits-snap-term");
    let store = SnapshotStore::for_home(&home);
    let mut dto = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
    dto.reading.subscription = term(false);
    store.save(&dto).unwrap();
    assert_eq!(
        store.load(AgentId::Codex)[0].reading.subscription,
        term(false)
    );

    let old = serde_json::json!({
        "schemaVersion": 2, "provider": "codex",
        "account": {"id": "acct-2", "label": "b@x"},
        "windows": [{"id": "primary", "label": "Weekly", "kind": "weekly", "usedPercent": 10.0,
                     "observedAt": "2026-08-17T10:00:00.000Z"}]
    });
    std::fs::write(
        home.join(".on-n-off/limits/codex-acct_2-00000000.json"),
        old.to_string(),
    )
    .unwrap();
    let loaded = store.load(AgentId::Codex);
    let older = loaded
        .iter()
        .find(|dto| dto.account.as_ref().unwrap().id == "acct-2")
        .expect("a snapshot written before the field loads");
    assert_eq!(older.reading.subscription, None);
    let _ = std::fs::remove_dir_all(&home);
}

/// A remembered Codex file as a version before the reader dropped hidden windows wrote it: the
/// weekly window beside `extra` windows, all observed at one time.
fn codex_file_with(store: &SnapshotStore, account: &str, extra: &[(&str, &str)]) -> PathBuf {
    let window = |id: &str, label: &str, kind: &str| {
        serde_json::json!({"id": id, "label": label, "kind": kind, "usedPercent": 100,
            "resetsAt": "2026-08-24T23:34:33+00:00", "observedAt": "2026-08-17T10:00:00.000Z"})
    };
    let mut windows = vec![window("primary", "Weekly · all models", "weekly")];
    windows.extend(extra.iter().map(|(id, label)| window(id, label, "model")));
    fs::create_dir_all(store.dir()).unwrap();
    let path = store.dir().join(file_name(AgentId::Codex, account));
    let stored = serde_json::json!({"schemaVersion": 2, "provider": "codex",
        "account": {"id": account, "label": "a@x"}, "plan": "pro", "windows": windows,
        "observedAt": "2026-08-17T10:00:00.000Z"});
    fs::write(&path, stored.to_string()).unwrap();
    path
}

/// Files written before the Codex reader dropped hidden windows still hold some. Loading drops
/// them through the reader's own rule, by bucket id and by name, so a paused read's last observed
/// values never bring one back. The file itself loses them only at its next save.
#[test]
fn a_remembered_codex_file_loses_its_hidden_windows_on_load() {
    let home = scratch_dir("limits-snap-hidden-windows");
    let store = SnapshotStore::for_home(&home);
    let path = codex_file_with(
        &store,
        "acct-1",
        &[
            ("extra:codex_bengalfox", "5 hour · GPT-5.3-Codex-Spark"),
            ("extra:base_model_inference:secondary", "Weekly · Inference"),
            ("extra:reserve", "Weekly · GPT-Reserve"),
            ("extra:gpt_luna", "Weekly · GPT-5.6-Luna"),
        ],
    );

    let remembered = store.load(AgentId::Codex).remove(0);
    let ids = |card: &ProviderLimitsDto| -> Vec<String> {
        card.reading
            .windows
            .iter()
            .map(|window| window.id.clone())
            .collect()
    };
    assert_eq!(ids(&remembered), ["primary", "extra:gpt_luna"]);

    let mut paused = ProviderLimitsDto {
        status: LimitsStatus::Failed,
        message: Some("Refresh failed.".into()),
        ..ProviderLimitsDto::for_test(AgentId::Codex, "acct-1")
    };
    crate::limits::keep_remembered(&mut paused, remembered.reading);
    assert_eq!(ids(&paused), ["primary", "extra:gpt_luna"]);
    assert!(fs::read_to_string(path)
        .unwrap()
        .contains("codex_bengalfox"));
}

/// A remembered Codex file holding only hidden windows has observed nothing a surface shows, so it
/// is not loaded, as none would be saved.
#[test]
fn a_remembered_codex_file_with_only_hidden_windows_is_not_loaded() {
    let home = scratch_dir("limits-snap-only-hidden-windows");
    let store = SnapshotStore::for_home(&home);
    let path = codex_file_with(
        &store,
        "acct-1",
        &[("extra:codex_bengalfox", "5 hour · GPT-5.3-Codex-Spark")],
    );
    let mut stored: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    stored["windows"].as_array_mut().unwrap().remove(0);
    fs::write(&path, stored.to_string()).unwrap();

    assert!(store.load(AgentId::Codex).is_empty());
}

/// The hidden-window rule is Codex's: another provider's remembered windows load whatever they are
/// called.
#[test]
fn another_providers_remembered_windows_load_whatever_their_names() {
    let home = scratch_dir("limits-snap-hidden-rule-is-codexs");
    let store = SnapshotStore::for_home(&home);
    let mut claude = snapshot(AgentId::Claude, "user-1", "a@x", "2026-08-17T10:00:00.000Z");
    let mut named_like_hidden = claude.reading.windows[0].clone();
    named_like_hidden.id = "extra:codex_bengalfox".into();
    named_like_hidden.label = "Weekly · GPT-Reserve".into();
    named_like_hidden.kind = LimitWindowKind::Model;
    claude.reading.windows.push(named_like_hidden);
    store.save(&claude).unwrap();

    assert_eq!(store.load(AgentId::Claude)[0].reading.windows.len(), 2);
}
