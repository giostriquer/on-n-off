use super::snapshot;
use crate::dto::{AgentId, LimitsAccountDto};
use crate::limits::snapshots::SnapshotStore;
use crate::paths::scratch_dir;
use std::collections::BTreeSet;
use std::path::Path;

fn legacy(id: &str, email: &str) -> LimitsAccountDto {
    LimitsAccountDto {
        id: id.into(),
        legacy_id: None,
        label: Some(email.into()),
    }
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn held(store: &SnapshotStore) -> Vec<String> {
    let mut held: Vec<String> = store
        .load(AgentId::Codex)
        .into_iter()
        .filter_map(|card| card.account.map(|account| account.id))
        .collect();
    held.sort();
    held
}

fn stored(home: &Path, cards: &[(&str, &str)]) -> SnapshotStore {
    let store = SnapshotStore::for_home(home);
    for (id, email) in cards {
        store
            .save(&snapshot(AgentId::Codex, id, email, "2026-09-13T12:00:00Z"))
            .unwrap();
    }
    store
}

#[test]
fn forgetting_a_card_removes_its_snapshot_every_history_it_replaced_and_their_archive_entries() {
    let home = scratch_dir("limits-forget-card");
    let store = stored(
        &home,
        &[
            ("profile:team", "shared@example.com"),
            ("team", "shared@example.com"),
            ("other", "other@example.com"),
        ],
    );
    store
        .set_archived(
            AgentId::Codex,
            &ids(&["profile:team", "team", "other"]),
            true,
        )
        .unwrap();

    let forgotten = store.forget_card(
        AgentId::Codex,
        "profile:team",
        &[legacy("team", " Shared@Example.com ")],
    );

    assert_eq!(forgotten, Ok(ids(&["team", "profile:team"])));
    assert_eq!(held(&store), ["other"]);
    assert_eq!(
        store.archived(AgentId::Codex),
        BTreeSet::from(["other".to_string()])
    );
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_card_whose_replaced_history_changed_hands_forgets_nothing() {
    let home = scratch_dir("limits-forget-card-changed");
    let store = stored(
        &home,
        &[
            ("profile:team", "shared@example.com"),
            ("team", "second@example.com"),
        ],
    );
    store
        .set_archived(AgentId::Codex, &ids(&["profile:team", "team"]), true)
        .unwrap();

    let refused = store.forget_card(
        AgentId::Codex,
        "profile:team",
        &[legacy("team", "shared@example.com")],
    );

    assert_eq!(
        refused,
        Err("Account history changed. Refresh accounts before removing it.".to_string())
    );
    assert_eq!(held(&store), ["profile:team", "team"]);
    assert_eq!(store.archived(AgentId::Codex).len(), 2);
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_card_with_no_history_on_disk_still_leaves_the_archive() {
    let home = scratch_dir("limits-forget-card-unread");
    let store = SnapshotStore::for_home(&home);
    store
        .set_archived(AgentId::Codex, &ids(&["profile:unread"]), true)
        .unwrap();

    assert_eq!(
        store.forget_card(AgentId::Codex, "profile:unread", &[]),
        Ok(ids(&["profile:unread"]))
    );
    assert!(store.archived(AgentId::Codex).is_empty());
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_replaced_history_with_no_email_to_check_is_never_deleted_unchecked() {
    let home = scratch_dir("limits-forget-card-unlabeled");
    let store = stored(
        &home,
        &[
            ("profile:team", "shared@example.com"),
            ("team", "shared@example.com"),
        ],
    );
    let unlabeled = LimitsAccountDto {
        id: "team".into(),
        legacy_id: None,
        label: None,
    };

    assert_eq!(
        store.forget_card(AgentId::Codex, "profile:team", &[unlabeled]),
        Err("Account history changed. Refresh accounts before removing it.".to_string())
    );
    assert_eq!(held(&store), ["profile:team", "team"]);
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn checking_a_cards_history_passes_what_still_matches_and_refuses_what_changed_hands() {
    let home = scratch_dir("limits-check-card-history");
    let store = stored(
        &home,
        &[
            ("team", "shared@example.com"),
            ("other", "second@example.com"),
        ],
    );

    assert_eq!(
        store.check_card_history(
            AgentId::Codex,
            &[
                legacy("team", " Shared@Example.com "),
                legacy("gone", "any@example.com")
            ]
        ),
        Ok(()),
        "a match, and history already gone"
    );
    assert!(store
        .check_card_history(AgentId::Codex, &[legacy("other", "shared@example.com")])
        .is_err());
    assert_eq!(held(&store), ["other", "team"], "checking deletes nothing");
    std::fs::remove_dir_all(home).unwrap();
}
