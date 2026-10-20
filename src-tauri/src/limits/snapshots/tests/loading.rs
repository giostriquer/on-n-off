use super::*;
use crate::provider_catalogue::OnLoad;

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
        resets: Vec::new(),
    });
    store.save(&dto).unwrap();
    assert_eq!(
        store.load(AgentId::Codex)[0].reading.reset_credits,
        dto.reading.reset_credits
    );

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
            resets: Vec::new(),
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
        resets: Vec::new(),
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

#[test]
fn a_remembered_reset_count_is_dropped_as_the_snapshot_loads_exactly_where_the_provider_says() {
    for provider in crate::provider_catalogue::WITH_LIMITS {
        let home = scratch_dir(&format!("limits-snap-resets-{}", provider.key()));
        let store = SnapshotStore::for_home(&home);
        let with_windows = serde_json::json!({
            "schemaVersion": 2, "provider": provider,
            "account": {"id": "acct-1", "label": "a@x"},
            "resetCredits": {"availableCount": 1},
            "windows": [{"id": "seven_day", "label": "Weekly", "kind": "weekly", "usedPercent": 10.0,
                         "observedAt": "2026-08-17T10:00:00.000Z"}]
        });
        let only_resets = serde_json::json!({
            "schemaVersion": 2, "provider": provider,
            "account": {"id": "acct-2", "label": "b@x"},
            "resetCredits": {"availableCount": 1},
            "windows": []
        });
        std::fs::create_dir_all(home.join(".on-n-off/limits")).unwrap();
        for (name, snapshot) in [("acct-1", with_windows), ("acct-2", only_resets)] {
            std::fs::write(
                home.join(".on-n-off/limits")
                    .join(file_name(provider, name)),
                snapshot.to_string(),
            )
            .unwrap();
        }

        let mut loaded = store.load(provider);
        loaded.sort_by_key(|card| card.account.as_ref().map(|account| account.id.clone()));

        let dropped = provider
            .limits_facts()
            .on_load
            .contains(&OnLoad::DropResetCredits);
        let ids: Vec<&str> = loaded
            .iter()
            .filter_map(|card| card.account.as_ref().map(|account| account.id.as_str()))
            .collect();
        let expected_ids: &[&str] = if dropped {
            &["acct-1"]
        } else {
            &["acct-1", "acct-2"]
        };
        assert_eq!(ids, expected_ids, "{provider:?}");
        assert_eq!(
            loaded[0].reading.reset_credits.is_none(),
            dropped,
            "{provider:?}"
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}

#[test]
fn a_snapshot_with_the_retired_subscription_status_still_loads() {
    let home = scratch_dir("limits-snap-subscription-status");
    let store = SnapshotStore::for_home(&home);
    let old = serde_json::json!({
        "schemaVersion": 2, "provider": "claude",
        "account": {"id": "acct-2", "label": "b@x"},
        "subscriptionStatus": "past_due",
        "windows": [{"id": "seven_day", "label": "Weekly", "kind": "weekly", "usedPercent": 10.0,
                     "observedAt": "2026-08-17T10:00:00.000Z"}]
    });
    std::fs::create_dir_all(home.join(".on-n-off/limits")).unwrap();
    std::fs::write(
        home.join(".on-n-off/limits/claude-acct_2-00000000.json"),
        old.to_string(),
    )
    .unwrap();

    let loaded = store.load(AgentId::Claude);

    let older = loaded
        .iter()
        .find(|dto| dto.account.as_ref().unwrap().id == "acct-2")
        .expect("a snapshot with the retired field loads");
    assert_eq!(older.reading.windows.len(), 1);
    let _ = std::fs::remove_dir_all(&home);
}

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
            ("extra:team_split", "Weekly · Team · GPT-Reserve"),
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
    assert_eq!(
        ids(&remembered),
        ["primary", "extra:gpt_luna", "extra:team_split"]
    );

    let mut paused = ProviderLimitsDto {
        status: LimitsStatus::Failed,
        message: Some("Refresh failed.".into()),
        ..ProviderLimitsDto::for_test(AgentId::Codex, "acct-1")
    };
    crate::limits::keep_remembered(&mut paused, remembered.reading);
    assert_eq!(
        ids(&paused),
        ["primary", "extra:gpt_luna", "extra:team_split"]
    );
    assert!(fs::read_to_string(path)
        .unwrap()
        .contains("codex_bengalfox"));
}

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

#[test]
fn only_a_provider_whose_files_drop_hidden_windows_loses_windows_named_like_them() {
    for provider in crate::provider_catalogue::WITH_LIMITS {
        let home = scratch_dir(&format!("limits-snap-hidden-rule-{}", provider.key()));
        let store = SnapshotStore::for_home(&home);
        let mut card = snapshot(provider, "user-1", "a@x", "2026-08-17T10:00:00.000Z");
        let mut named_like_hidden = card.reading.windows[0].clone();
        named_like_hidden.id = "extra:codex_bengalfox".into();
        named_like_hidden.label = "Weekly · GPT-Reserve".into();
        named_like_hidden.kind = LimitWindowKind::Model;
        card.reading.windows.push(named_like_hidden);
        store.save(&card).unwrap();

        let hides = provider
            .limits_facts()
            .on_load
            .contains(&OnLoad::DropHiddenWindows);
        assert_eq!(
            store.load(provider)[0].reading.windows.len(),
            if hides { 1 } else { 2 },
            "{provider:?}"
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}

#[test]
fn a_remembered_file_out_of_range_loads_in_range_and_stays_as_written() {
    let home = scratch_dir("limits-snap-out-of-range");
    let store = SnapshotStore::for_home(&home);
    let path = codex_file_with(
        &store,
        "acct-1",
        &[("extra:gpt_luna", "Weekly · GPT-5.6-Luna")],
    );
    let mut stored: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    stored["windows"][0]["usedPercent"] = serde_json::json!(140.0);
    stored["windows"][1]["usedPercent"] = serde_json::json!(-3.0);
    stored["workspaceCredits"] = serde_json::json!({"limit": "100", "used": "250",
        "usedPercent": 250.0, "reached": true});
    fs::write(&path, stored.to_string()).unwrap();

    let remembered = store.load(AgentId::Codex).remove(0);

    let used: Vec<f64> = remembered
        .reading
        .windows
        .iter()
        .map(|window| window.used_percent)
        .collect();
    assert_eq!(used, [100.0, 0.0]);
    assert_eq!(
        remembered
            .reading
            .workspace_credits
            .map(|share| share.used_percent),
        Some(100.0)
    );
    assert!(fs::read_to_string(&path).unwrap().contains("140"));
}

#[test]
fn a_file_written_before_the_first_message_flag_marks_an_idle_session_where_its_provider_says() {
    for provider in crate::provider_catalogue::WITH_LIMITS {
        let home = scratch_dir(&format!("limits-snap-first-message-{}", provider.key()));
        let store = SnapshotStore::for_home(&home);
        let window = |id: &str, kind: &str, used: f64, resets_at: Option<&str>| {
            let mut window = serde_json::json!({"id": id, "label": id, "kind": kind,
                "usedPercent": used, "observedAt": "2026-08-17T10:00:00.000Z"});
            if let Some(at) = resets_at {
                window["resetsAt"] = serde_json::json!(at);
            }
            window
        };
        let old_shape = serde_json::json!({
            "schemaVersion": 2, "provider": provider,
            "account": {"id": "acct-1", "label": "a@x"},
            "windows": [
                window("weekly", "weekly", 0.0, None),
                window("idle", "session", 0.0, None),
                window("used", "session", 4.0, None),
                window("started", "session", 0.0, Some("2026-08-17T15:00:00Z")),
                window("model", "model", 0.0, None)
            ],
            "observedAt": "2026-08-17T10:00:00.000Z"
        });
        fs::create_dir_all(store.dir()).unwrap();
        let path = store.dir().join(file_name(provider, "acct-1"));
        fs::write(&path, old_shape.to_string()).unwrap();

        let loaded = store.load(provider).remove(0);

        let marks = provider
            .limits_facts()
            .on_load
            .contains(&OnLoad::MarkFirstMessageSessions);
        let flagged: Vec<(&str, bool)> = loaded
            .reading
            .windows
            .iter()
            .map(|window| (window.id.as_str(), window.starts_on_first_message))
            .collect();
        assert_eq!(
            flagged,
            [
                ("weekly", false),
                ("idle", marks),
                ("used", false),
                ("started", false),
                ("model", false)
            ],
            "{provider:?}"
        );
        assert!(!fs::read_to_string(&path)
            .unwrap()
            .contains("startsOnFirstMessage"));
        let _ = fs::remove_dir_all(&home);
    }
}
