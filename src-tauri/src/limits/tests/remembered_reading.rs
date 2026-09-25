//! What the signed-in read keeps of the account's remembered reading, whole: on the card it shows
//! and on disk, after a read that answered and after one that failed. Inputs and expectations are
//! wire JSON, so they hold whatever Rust types carry the reading.

use super::*;
use serde_json::Value;

fn card(value: Value) -> ProviderLimitsDto {
    serde_json::from_value(value).expect("a card")
}

fn wire(dto: &ProviderLimitsDto) -> Value {
    serde_json::to_value(dto).unwrap()
}

/// `acct-1`'s remembered Codex reading on `plan`, observed at 10:00 with every figure known.
fn remembered(plan: &str) -> ProviderLimitsDto {
    card(json!({
        "provider": "codex",
        "status": "ok",
        "account": {"id": "acct-1", "label": "a@example.com"},
        "currentAccount": true,
        "plan": plan,
        "subscriptionStatus": "active",
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

/// A read that answered with its plan, one window and an offer, and nothing else.
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

/// A read that answered without a plan says the account has none now: the card and the file lose
/// the remembered plan, and with it what was spent, which only a workspace plan is asked.
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

/// A failed read shows the remembered reading under its failure. What was spent is kept on any
/// plan, since the failure could not say the plan changed.
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
            "subscriptionStatus": "active",
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

/// A failed read that still carries windows merges them with the remembered ones by id: the newer
/// observation of each wins, one whose time cannot be read is never replaced, a remembered window
/// the read lacks is added, and a remembered window without a time takes the reading's newest.
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

/// A remembered reading whose windows carry no time it can be merged by is not shown at all, not
/// even its plan or its figures.
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
