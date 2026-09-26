//! A snapshot file key for key: what released versions wrote and load, so a change to the Rust
//! types that hold a reading cannot move a key. Inputs are wire JSON for the same reason.

use super::*;
use serde_json::{json, Value};

/// The one snapshot file `store` holds, as JSON.
fn the_file(store: &SnapshotStore) -> Value {
    let files: Vec<_> = fs::read_dir(store.dir())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    serde_json::from_str(&fs::read_to_string(&files[0]).unwrap()).unwrap()
}

fn card(value: Value) -> ProviderLimitsDto {
    serde_json::from_value(value).expect("a card")
}

/// A Codex business member's successful read with every figure known, and a live offer beside them.
fn every_figure() -> ProviderLimitsDto {
    card(json!({
        "provider": "codex",
        "status": "ok",
        "account": {"id": "profile:acct-1", "legacyId": "acct-1", "label": "a@example.com"},
        "currentAccount": true,
        "plan": "business",
        "subscriptionStatus": "active",
        "windows": [
            {"id": "primary", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 42.0,
             "resetsAt": "2026-08-24T23:34:33+00:00", "windowSeconds": 604800,
             "observedAt": "2026-08-17T10:00:00.000Z"},
            {"id": "secondary", "label": "5 hour · all models", "kind": "session", "usedPercent": 7.5,
             "observedAt": "2026-08-17T12:00:00.000+01:00"}
        ],
        "credits": {"balance": "3", "unlimited": false},
        "workspaceCredits": {"limit": "25000", "used": "8000", "usedPercent": 32.0,
                             "resetsAt": "2100-10-01T12:00:00+00:00", "reached": true},
        "creditsSpent": {"last7Days": 18303.4, "last30Days": 20299.7,
                         "updatedAt": "2026-08-17T09:00:00Z"},
        "subscription": {"activeUntil": "2100-09-28T16:22:34Z", "willRenew": false,
                         "note": "cancelled", "checkedAt": "2026-08-17T10:00:00Z"},
        "resetCredits": {"availableCount": 1, "nextExpiresAt": "2100-09-01T12:00:00+00:00"},
        "resetOffer": {"price": {"amountMinorUnits": 800, "currency": "USD"}}
    }))
}

/// Every figure under its wire name, the schema version, and the newest window's time in UTC
/// milliseconds; never the offer, the status or whether the account is signed in.
#[test]
fn a_snapshot_file_holds_the_whole_reading_under_its_wire_names() {
    let home = scratch_dir("limits-snap-file-shape");
    let store = SnapshotStore::for_home(&home);

    store.save(&every_figure()).unwrap();

    assert_eq!(
        the_file(&store),
        json!({
            "schemaVersion": 2,
            "provider": "codex",
            "account": {"id": "profile:acct-1", "legacyId": "acct-1", "label": "a@example.com"},
            "plan": "business",
            "subscriptionStatus": "active",
            "windows": [
                {"id": "primary", "label": "Weekly · all models", "kind": "weekly",
                 "usedPercent": 42.0, "resetsAt": "2026-08-24T23:34:33+00:00",
                 "windowSeconds": 604800, "observedAt": "2026-08-17T10:00:00.000Z"},
                {"id": "secondary", "label": "5 hour · all models", "kind": "session",
                 "usedPercent": 7.5, "observedAt": "2026-08-17T12:00:00.000+01:00"}
            ],
            "credits": {"balance": "3", "unlimited": false},
            "workspaceCredits": {"limit": "25000", "used": "8000", "usedPercent": 32.0,
                                 "resetsAt": "2100-10-01T12:00:00+00:00", "reached": true},
            "creditsSpent": {"last7Days": 18303.4, "last30Days": 20299.7,
                             "updatedAt": "2026-08-17T09:00:00Z"},
            "subscription": {"activeUntil": "2100-09-28T16:22:34Z", "willRenew": false,
                             "note": "cancelled", "checkedAt": "2026-08-17T10:00:00Z"},
            "resetCredits": {"availableCount": 1, "nextExpiresAt": "2100-09-01T12:00:00+00:00"},
            "observedAt": "2026-08-17T11:00:00.000Z"
        })
    );
    let _ = fs::remove_dir_all(&home);
}

/// Whether a card is a saved profile's describes the read that showed it, so its file never says,
/// and the card it loads back is a remembered reading.
#[test]
fn a_saved_profiles_card_is_filed_without_saying_so() {
    let home = scratch_dir("limits-snap-file-shape-saved-profile");
    let store = SnapshotStore::for_home(&home);
    let saved = ProviderLimitsDto {
        current_account: false,
        saved_profile: true,
        ..every_figure()
    };

    store.save(&saved).unwrap();

    assert!(the_file(&store).get("savedProfile").is_none());
    assert!(!store.load(AgentId::Codex)[0].saved_profile);
    let _ = fs::remove_dir_all(&home);
}

/// A newer read that told only its plan and one window: the account details and balances it did
/// not report are gone, its windows replace the old ones, and the figures it could not tell stay.
#[test]
fn a_newer_read_that_could_not_tell_keeps_only_the_remembered_figures_in_the_file() {
    let home = scratch_dir("limits-snap-file-shape-kept");
    let store = SnapshotStore::for_home(&home);
    store.save(&every_figure()).unwrap();
    let newer = card(json!({
        "provider": "codex",
        "status": "ok",
        "account": {"id": "profile:acct-1", "legacyId": "acct-1", "label": "a@example.com"},
        "currentAccount": true,
        "plan": "business",
        "windows": [
            {"id": "primary", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 60.0,
             "observedAt": "2026-08-17T13:00:00.000Z"}
        ]
    }));

    store.remember(newer).saved.unwrap();

    assert_eq!(
        the_file(&store),
        json!({
            "schemaVersion": 2,
            "provider": "codex",
            "account": {"id": "profile:acct-1", "legacyId": "acct-1", "label": "a@example.com"},
            "plan": "business",
            "windows": [
                {"id": "primary", "label": "Weekly · all models", "kind": "weekly",
                 "usedPercent": 60.0, "observedAt": "2026-08-17T13:00:00.000Z"}
            ],
            "creditsSpent": {"last7Days": 18303.4, "last30Days": 20299.7,
                             "updatedAt": "2026-08-17T09:00:00Z"},
            "subscription": {"activeUntil": "2100-09-28T16:22:34Z", "willRenew": false,
                             "note": "cancelled", "checkedAt": "2026-08-17T10:00:00Z"},
            "resetCredits": {"availableCount": 1, "nextExpiresAt": "2100-09-01T12:00:00+00:00"},
            "observedAt": "2026-08-17T13:00:00.000Z"
        })
    );
    let _ = fs::remove_dir_all(&home);
}

/// A remembered count whose soonest expiry has passed is not known any more, so a newer read that
/// could not tell the count leaves none in the file, as none is loaded from it.
#[test]
fn a_newer_read_that_could_not_tell_leaves_no_lapsed_count_in_the_file() {
    let home = scratch_dir("limits-snap-file-shape-lapsed");
    let store = SnapshotStore::for_home(&home);
    let read = |observed_at: &str, reset_credits: Value| {
        card(json!({
            "provider": "codex",
            "status": "ok",
            "account": {"id": "acct-1"},
            "currentAccount": true,
            "windows": [
                {"id": "primary", "label": "Weekly · all models", "kind": "weekly",
                 "usedPercent": 60.0, "observedAt": observed_at}
            ],
            "resetCredits": reset_credits
        }))
    };
    store
        .save(&read(
            "2026-08-17T10:00:00.000Z",
            json!({"availableCount": 2, "nextExpiresAt": "2020-01-01T00:00:00+00:00"}),
        ))
        .unwrap();

    store
        .remember(read("2026-08-17T11:00:00.000Z", Value::Null))
        .saved
        .unwrap();

    assert_eq!(the_file(&store).get("resetCredits"), None);
    let _ = fs::remove_dir_all(&home);
}

/// No version writes an offer to a file, and one that is there anyway is never read back: an offer
/// is withdrawn the moment the account is under its limit again.
#[test]
fn an_offer_in_a_snapshot_file_is_not_loaded() {
    let home = scratch_dir("limits-snap-file-shape-offer");
    let store = SnapshotStore::for_home(&home);
    fs::create_dir_all(store.dir()).unwrap();
    fs::write(
        store.dir().join("codex-acct_1-00000000.json"),
        json!({
            "schemaVersion": 2,
            "provider": "codex",
            "account": {"id": "acct-1"},
            "windows": [
                {"id": "primary", "label": "Weekly · all models", "kind": "weekly",
                 "usedPercent": 40.0, "observedAt": "2026-08-17T10:00:00.000Z"}
            ],
            "resetOffer": {"price": {"amountMinorUnits": 800, "currency": "USD"}}
        })
        .to_string(),
    )
    .unwrap();

    let loaded = store.load(AgentId::Codex);

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].reading.reset_offer, None);
    let _ = fs::remove_dir_all(&home);
}
