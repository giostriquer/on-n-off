use super::*;
use serde_json::Value;

fn card(value: Value) -> ProviderLimitsDto {
    serde_json::from_value(value).expect("a card")
}

fn wire(dto: &ProviderLimitsDto) -> Value {
    serde_json::to_value(dto).unwrap()
}

fn remembered(plan: &str) -> ProviderLimitsDto {
    card(json!({
        "provider": "codex",
        "status": "ok",
        "account": {"id": "acct-1", "label": "a@example.com"},
        "currentAccount": true,
        "plan": plan,
        "windows": [
            {"id": "primary", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 40.0,
             "resetsAt": "2026-08-24T10:00:00Z", "observedAt": "2026-08-17T10:00:00.000Z"},
            {"id": "secondary", "label": "5 hour · all models", "kind": "session", "usedPercent": 17.0,
             "observedAt": "2026-08-17T10:00:00.000Z"}
        ],
        "credits": {"balance": "0", "unlimited": false},
        "workspaceCredits": {"limit": "25000", "used": "8000", "usedPercent": 32.0, "reached": false},
        "creditsSpent": {"last7Days": 18303.4, "last30Days": 20303.4},
        "subscription": {"activeUntil": "2100-09-28T16:22:34Z", "willRenew": false,
                         "note": "cancelled", "checkedAt": "2026-08-17T10:00:00Z"},
        "resetCredits": {"availableCount": 1, "nextExpiresAt": "2100-09-01T12:00:00+00:00"}
    }))
}

#[test]
fn an_answered_read_keeps_only_the_remembered_figures_it_could_not_tell() {
    let home = scratch_dir("limits-reading-answered");
    let store = SnapshotStore::for_home(&home);
    store.save(&remembered("business")).unwrap();
    let answered = card(json!({
        "provider": "codex",
        "status": "ok",
        "account": {"id": "acct-1", "label": "a@example.com"},
        "currentAccount": true,
        "plan": "business",
        "windows": [
            {"id": "primary", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 50.0,
             "observedAt": "2026-08-17T11:00:00.000Z"}
        ],
        "resetOffer": {"price": {"amountMinorUnits": 800, "currency": "USD"}}
    }));

    let listed = aggregate_accounts(&store, answered);

    assert_eq!(listed.len(), 1);
    assert_eq!(
        wire(&listed[0]),
        json!({
            "provider": "codex",
            "status": "ok",
            "account": {"id": "acct-1", "label": "a@example.com"},
            "currentAccount": true,
            "plan": "business",
            "windows": [
                {"id": "primary", "label": "Weekly · all models", "kind": "weekly",
                 "usedPercent": 50.0, "observedAt": "2026-08-17T11:00:00.000Z"}
            ],
            "creditsSpent": {"last7Days": 18303.4, "last30Days": 20303.4},
            "subscription": {"activeUntil": "2100-09-28T16:22:34Z", "willRenew": false,
                             "note": "cancelled", "checkedAt": "2026-08-17T10:00:00Z"},
            "resetCredits": {"availableCount": 1, "nextExpiresAt": "2100-09-01T12:00:00+00:00"},
            "resetOffer": {"price": {"amountMinorUnits": 800, "currency": "USD"}}
        })
    );
    assert_eq!(
        wire(&store.load(AgentId::Codex)[0]),
        json!({
            "provider": "codex",
            "status": "ok",
            "account": {"id": "acct-1", "label": "a@example.com"},
            "currentAccount": false,
            "plan": "business",
            "windows": [
                {"id": "primary", "label": "Weekly · all models", "kind": "weekly",
                 "usedPercent": 50.0, "observedAt": "2026-08-17T11:00:00.000Z"}
            ],
            "creditsSpent": {"last7Days": 18303.4, "last30Days": 20303.4},
            "subscription": {"activeUntil": "2100-09-28T16:22:34Z", "willRenew": false,
                             "note": "cancelled", "checkedAt": "2026-08-17T10:00:00Z"},
            "resetCredits": {"availableCount": 1, "nextExpiresAt": "2100-09-01T12:00:00+00:00"}
        })
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn an_answered_read_without_a_plan_drops_the_remembered_plan() {
    let home = scratch_dir("limits-reading-answered-no-plan");
    let store = SnapshotStore::for_home(&home);
    store.save(&remembered("business")).unwrap();
    let answered = card(json!({
        "provider": "codex",
        "status": "ok",
        "account": {"id": "acct-1", "label": "a@example.com"},
        "currentAccount": true,
        "windows": [
            {"id": "primary", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 50.0,
             "observedAt": "2026-08-17T11:00:00.000Z"}
        ]
    }));

    let listed = aggregate_accounts(&store, answered);

    let expected = json!({
        "provider": "codex",
        "status": "ok",
        "account": {"id": "acct-1", "label": "a@example.com"},
        "currentAccount": true,
        "windows": [
            {"id": "primary", "label": "Weekly · all models", "kind": "weekly",
             "usedPercent": 50.0, "observedAt": "2026-08-17T11:00:00.000Z"}
        ],
        "subscription": {"activeUntil": "2100-09-28T16:22:34Z", "willRenew": false,
                         "note": "cancelled", "checkedAt": "2026-08-17T10:00:00Z"},
        "resetCredits": {"availableCount": 1, "nextExpiresAt": "2100-09-01T12:00:00+00:00"}
    });
    assert_eq!(wire(&listed[0]), expected);
    let mut loaded = wire(&store.load(AgentId::Codex)[0]);
    loaded["currentAccount"] = json!(true);
    assert_eq!(loaded, expected, "the file loses the plan too");
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn a_failed_read_shows_the_remembered_reading() {
    let home = scratch_dir("limits-reading-failed");
    let store = SnapshotStore::for_home(&home);
    store.save(&remembered("pro")).unwrap();
    let failed = card(json!({
        "provider": "codex",
        "status": "failed",
        "message": "Refresh failed",
        "account": {"id": "acct-1", "label": "a@example.com"},
        "currentAccount": true,
        "windows": []
    }));

    let listed = aggregate_accounts(&store, failed);

    assert_eq!(listed.len(), 1);
    assert_eq!(
        wire(&listed[0]),
        json!({
            "provider": "codex",
            "status": "failed",
            "message": "Refresh failed",
            "account": {"id": "acct-1", "label": "a@example.com"},
            "currentAccount": true,
            "plan": "pro",
            "windows": [
                {"id": "primary", "label": "Weekly · all models", "kind": "weekly",
                 "usedPercent": 40.0, "resetsAt": "2026-08-24T10:00:00Z",
                 "observedAt": "2026-08-17T10:00:00.000Z"},
                {"id": "secondary", "label": "5 hour · all models", "kind": "session",
                 "usedPercent": 17.0, "observedAt": "2026-08-17T10:00:00.000Z"}
            ],
            "credits": {"balance": "0", "unlimited": false},
            "workspaceCredits": {"limit": "25000", "used": "8000", "usedPercent": 32.0,
                                 "reached": false},
            "creditsSpent": {"last7Days": 18303.4, "last30Days": 20303.4},
            "subscription": {"activeUntil": "2100-09-28T16:22:34Z", "willRenew": false,
                             "note": "cancelled", "checkedAt": "2026-08-17T10:00:00Z"},
            "resetCredits": {"availableCount": 1, "nextExpiresAt": "2100-09-01T12:00:00+00:00"}
        })
    );
    assert_eq!(
        wire(&store.load(AgentId::Codex)[0]),
        wire(&ProviderLimitsDto {
            current_account: false,
            ..remembered("pro")
        }),
        "the failure rewrites the remembered reading as it was"
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn a_failed_read_shows_every_remembered_window() {
    let home = scratch_dir("limits-reading-failed-every-window");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&card(json!({
            "provider": "claude",
            "status": "ok",
            "account": {"id": "uuid-1"},
            "currentAccount": true,
            "windows": [
                {"id": "weekly_opus", "label": "Weekly · Opus", "kind": "model", "usedPercent": 91.0,
                 "observedAt": "2026-08-17T10:00:00.000Z"},
                {"id": "session", "label": "5 hour · all models", "kind": "session",
                 "usedPercent": 17.0, "observedAt": "2026-08-17T10:00:00.000Z"},
                {"id": "weekly_sonnet", "label": "Weekly · Sonnet", "kind": "model",
                 "usedPercent": 5.0, "observedAt": "2026-08-17T10:00:00.000Z"},
                {"id": "weekly_all", "label": "Weekly · all models", "kind": "weekly",
                 "usedPercent": 39.0, "observedAt": "2026-08-17T10:00:00.000Z"}
            ]
        })))
        .unwrap();
    let failed = card(json!({
        "provider": "claude",
        "status": "unauthenticated",
        "message": "Refresh paused",
        "account": {"id": "uuid-1"},
        "currentAccount": true,
        "windows": []
    }));

    let listed = aggregate_accounts(&store, failed);

    assert_eq!(
        wire(&listed[0])["windows"],
        json!([
            {"id": "weekly_all", "label": "Weekly · all models", "kind": "weekly",
             "usedPercent": 39.0, "observedAt": "2026-08-17T10:00:00.000Z"},
            {"id": "session", "label": "5 hour · all models", "kind": "session",
             "usedPercent": 17.0, "observedAt": "2026-08-17T10:00:00.000Z"},
            {"id": "weekly_opus", "label": "Weekly · Opus", "kind": "model", "usedPercent": 91.0,
             "observedAt": "2026-08-17T10:00:00.000Z"},
            {"id": "weekly_sonnet", "label": "Weekly · Sonnet", "kind": "model", "usedPercent": 5.0,
             "observedAt": "2026-08-17T10:00:00.000Z"}
        ])
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn a_failed_read_merges_its_windows_with_the_remembered_ones_by_id() {
    let home = scratch_dir("limits-reading-failed-windows");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&card(json!({
            "provider": "claude",
            "status": "ok",
            "account": {"id": "uuid-1"},
            "currentAccount": true,
            "windows": [
                {"id": "weekly_all", "label": "Weekly · all models", "kind": "weekly",
                 "usedPercent": 39.0, "resetsAt": "2026-08-24T12:00:00Z",
                 "observedAt": "2026-08-17T10:00:00.000Z"},
                {"id": "weekly_opus", "label": "Weekly · Opus", "kind": "model", "usedPercent": 91.0,
                 "observedAt": "2026-08-17T10:00:00.000Z"},
                {"id": "weekly_sonnet", "label": "Weekly · Sonnet", "kind": "model",
                 "usedPercent": 5.0, "observedAt": "2026-08-17T10:00:00.000Z"},
                {"id": "session", "label": "5 hour · all models", "kind": "session",
                 "usedPercent": 17.0, "observedAt": ""}
            ]
        })))
        .unwrap();
    let failed = card(json!({
        "provider": "claude",
        "status": "unauthenticated",
        "message": "Refresh paused",
        "account": {"id": "uuid-1"},
        "currentAccount": true,
        "windows": [
            {"id": "weekly_all", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 63.0,
             "observedAt": "2026-08-17T12:00:00.000Z"},
            {"id": "weekly_opus", "label": "Weekly · Opus", "kind": "model", "usedPercent": 10.0,
             "observedAt": "2026-08-17T08:00:00.000Z"},
            {"id": "weekly_sonnet", "label": "Weekly · Sonnet", "kind": "model", "usedPercent": 1.0,
             "observedAt": "not a time"}
        ]
    }));

    let listed = aggregate_accounts(&store, failed);

    assert_eq!(
        wire(&listed[0])["windows"],
        json!([
            {"id": "weekly_all", "label": "Weekly · all models", "kind": "weekly",
             "usedPercent": 63.0, "observedAt": "2026-08-17T12:00:00.000Z"},
            {"id": "session", "label": "5 hour · all models", "kind": "session",
             "usedPercent": 17.0, "observedAt": "2026-08-17T10:00:00.000Z"},
            {"id": "weekly_opus", "label": "Weekly · Opus", "kind": "model", "usedPercent": 91.0,
             "observedAt": "2026-08-17T10:00:00.000Z"},
            {"id": "weekly_sonnet", "label": "Weekly · Sonnet", "kind": "model", "usedPercent": 1.0,
             "observedAt": "not a time"}
        ])
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn a_failed_read_shows_nothing_of_a_remembered_reading_it_cannot_date() {
    let home = scratch_dir("limits-reading-failed-undated");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&card(json!({
            "provider": "codex",
            "status": "ok",
            "account": {"id": "acct-1"},
            "currentAccount": true,
            "plan": "pro",
            "windows": [
                {"id": "primary", "label": "Weekly · all models", "kind": "weekly",
                 "usedPercent": 40.0, "observedAt": ""}
            ],
            "credits": {"balance": "3", "unlimited": false}
        })))
        .unwrap();
    let failed = card(json!({
        "provider": "codex",
        "status": "failed",
        "message": "Refresh failed",
        "account": {"id": "acct-1"},
        "currentAccount": true,
        "windows": []
    }));

    let listed = aggregate_accounts(&store, failed);

    assert_eq!(
        wire(&listed[0]),
        json!({
            "provider": "codex",
            "status": "failed",
            "message": "Refresh failed",
            "account": {"id": "acct-1"},
            "currentAccount": true,
            "windows": []
        })
    );
    let _ = fs::remove_dir_all(&home);
}

fn file_of(store: &SnapshotStore, id: &str) -> Value {
    fs::read_dir(store.dir())
        .unwrap()
        .flatten()
        .map(|entry| serde_json::from_str::<Value>(&fs::read_to_string(entry.path()).unwrap()))
        .map(Result::unwrap)
        .find(|file| file["account"]["id"] == id)
        .expect("the account's file")
}

#[test]
fn a_legacy_keyed_card_keeps_from_its_own_file_even_when_the_list_hides_it() {
    let home = scratch_dir("limits-reading-legacy-keyed");
    let store = SnapshotStore::for_home(&home);
    store
        .save(&card(json!({
            "provider": "claude",
            "status": "ok",
            "account": {"id": "profile:x", "legacyId": "user-a", "label": "a@example.com"},
            "currentAccount": false,
            "windows": [
                {"id": "weekly_all", "label": "Weekly · all models", "kind": "weekly",
                 "usedPercent": 10.0, "observedAt": "2026-08-17T08:00:00.000Z"}
            ]
        })))
        .unwrap();
    store
        .save(&card(json!({
            "provider": "claude",
            "status": "ok",
            "account": {"id": "user-a", "label": "a@example.com"},
            "currentAccount": true,
            "windows": [
                {"id": "weekly_all", "label": "Weekly · all models", "kind": "weekly",
                 "usedPercent": 40.0, "observedAt": "2026-08-17T09:00:00.000Z"}
            ]
        })))
        .unwrap();
    let session = json!({"id": "session", "label": "5 hour · all models", "kind": "session",
                         "usedPercent": 7.0, "observedAt": "2026-08-17T10:00:00.000Z"});
    let answered = card(json!({
        "provider": "claude",
        "status": "ok",
        "account": {"id": "user-a", "label": "a@example.com"},
        "currentAccount": true,
        "windows": [session]
    }));

    let listed = aggregate_accounts(&store, answered);

    let weekly = json!({"id": "weekly_all", "label": "Weekly · all models", "kind": "weekly",
                        "usedPercent": 40.0, "observedAt": "2026-08-17T09:00:00.000Z"});
    assert_eq!(
        wire(&listed[0]),
        json!({
            "provider": "claude",
            "status": "ok",
            "account": {"id": "user-a", "label": "a@example.com"},
            "currentAccount": true,
            "windows": [weekly, session]
        })
    );
    let file = file_of(&store, "user-a");
    assert_eq!(file["windows"], json!([weekly, session]));
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn an_answer_keeps_the_term_of_its_own_file_whose_lapsed_count_hides_it() {
    let home = scratch_dir("limits-reading-lapsed-term");
    let store = SnapshotStore::for_home(&home);
    let term = json!({"activeUntil": "2100-09-28T16:22:34Z", "willRenew": false,
                      "note": "cancelled", "checkedAt": "2026-08-17T10:00:00Z"});
    store
        .save(&card(json!({
            "provider": "codex",
            "status": "ok",
            "account": {"id": "acct-1", "label": "a@example.com"},
            "currentAccount": false,
            "plan": "pro",
            "windows": [],
            "subscription": term,
            "resetCredits": {"availableCount": 2, "nextExpiresAt": "2020-01-01T00:00:00+00:00"}
        })))
        .unwrap();
    assert!(store.load(AgentId::Codex).is_empty(), "the file is hidden");
    let answered = card(json!({
        "provider": "codex",
        "status": "ok",
        "account": {"id": "acct-1", "label": "a@example.com"},
        "currentAccount": true,
        "plan": "pro",
        "windows": [
            {"id": "primary", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 50.0,
             "observedAt": "2026-08-17T11:00:00.000Z"}
        ]
    }));

    let listed = aggregate_accounts(&store, answered);

    assert_eq!(listed.len(), 1);
    assert_eq!(wire(&listed[0])["subscription"], term);
    assert_eq!(wire(&listed[0]).get("resetCredits"), None);
    let _ = fs::remove_dir_all(&home);
}
