//! The archive file: what it holds per provider, what a missing or malformed one reads as, that
//! writes are whole and serialized, that the snapshot loader never takes it for a snapshot, and
//! which ids Forget and an unarchived account take out of it.

use super::super::tests::snapshot;
use super::super::{is_snapshot_file, SnapshotStore};
use super::ARCHIVE_FILE;
use crate::dto::{AgentId, LimitsAccountDto};
use crate::paths::scratch_dir;
use std::collections::BTreeSet;
use std::fs;

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn set(values: &[&str]) -> BTreeSet<String> {
    ids(values).into_iter().collect()
}

fn the_file(store: &SnapshotStore) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(store.dir().join(ARCHIVE_FILE)).unwrap()).unwrap()
}

#[test]
fn nothing_is_archived_until_the_file_says_so() {
    let home = scratch_dir("limits-archive-missing");
    let store = SnapshotStore::for_home(&home);

    assert!(store.archived(AgentId::Claude).is_empty());
    assert!(!store.dir().exists(), "reading creates nothing");
    let _ = fs::remove_dir_all(home);
}

#[test]
fn each_provider_keeps_its_own_archived_ids_until_they_are_unarchived() {
    let home = scratch_dir("limits-archive-ids");
    let store = SnapshotStore::for_home(&home);

    assert_eq!(
        store.set_archived(AgentId::Claude, &ids(&["profile:a", "user-a"]), true),
        Ok(true)
    );
    assert_eq!(
        store.set_archived(AgentId::Codex, &ids(&["team"]), true),
        Ok(true)
    );
    assert_eq!(
        store.set_archived(AgentId::Claude, &ids(&["user-a"]), true),
        Ok(false),
        "archiving an archived id changes nothing"
    );
    assert_eq!(
        store.archived(AgentId::Claude),
        set(&["profile:a", "user-a"])
    );
    assert_eq!(store.archived(AgentId::Codex), set(&["team"]));

    assert_eq!(
        store.set_archived(AgentId::Claude, &ids(&["profile:a"]), false),
        Ok(true)
    );
    assert_eq!(
        store.set_archived(AgentId::Claude, &ids(&["profile:a"]), false),
        Ok(false),
        "unarchiving an id that is not archived changes nothing"
    );
    assert_eq!(store.archived(AgentId::Claude), set(&["user-a"]));
    assert_eq!(
        the_file(&store),
        serde_json::json!({"claude": ["user-a"], "codex": ["team"]})
    );
    let _ = fs::remove_dir_all(home);
}

#[test]
fn a_malformed_file_reads_as_nothing_archived_and_the_next_write_replaces_it() {
    let home = scratch_dir("limits-archive-malformed");
    let store = SnapshotStore::for_home(&home);
    fs::create_dir_all(store.dir()).unwrap();
    for garbage in ["not json", "{\"claude\": \"profile:a\"}", "[]"] {
        fs::write(store.dir().join(ARCHIVE_FILE), garbage).unwrap();

        assert!(store.archived(AgentId::Claude).is_empty(), "{garbage}");
        assert_eq!(
            store.set_archived(AgentId::Codex, &ids(&["team"]), true),
            Ok(true),
            "{garbage}"
        );
        assert_eq!(
            the_file(&store),
            serde_json::json!({"codex": ["team"]}),
            "{garbage}"
        );
    }
    let _ = fs::remove_dir_all(home);
}

/// The loader's own filter decides: no provider's snapshots include the archive, and a snapshot
/// beside it loads as it did.
#[test]
fn the_archive_is_never_mistaken_for_a_snapshot() {
    for provider in [
        AgentId::Claude,
        AgentId::Codex,
        AgentId::Antigravity,
        AgentId::Cursor,
    ] {
        assert!(!is_snapshot_file(provider, ARCHIVE_FILE), "{provider:?}");
    }
    let home = scratch_dir("limits-archive-isolated");
    let store = SnapshotStore::for_home(&home);
    let remembered = snapshot(
        AgentId::Codex,
        "team",
        "a@example.com",
        "2026-09-20T10:00:00Z",
    );
    store.save(&remembered).unwrap();
    store
        .set_archived(AgentId::Codex, &ids(&["team"]), true)
        .unwrap();

    let loaded = store.load(AgentId::Codex);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].account, remembered.account);
    assert_eq!(store.archived(AgentId::Codex), set(&["team"]));
    let _ = fs::remove_dir_all(home);
}

/// Writes share the snapshot lock and replace the file whole: writers racing on distinct ids lose
/// none of them, and nothing but the archive is left behind.
#[test]
fn concurrent_archive_writes_lose_no_id_and_leave_no_partial_file() {
    let home = scratch_dir("limits-archive-concurrent");
    let store = SnapshotStore::for_home(&home);
    let written: Vec<String> = (0..16).map(|n| format!("profile:{n:02}")).collect();

    std::thread::scope(|scope| {
        for id in &written {
            let store = &store;
            scope.spawn(move || {
                store
                    .set_archived(AgentId::Claude, std::slice::from_ref(id), true)
                    .unwrap();
            });
        }
    });

    assert_eq!(
        store.archived(AgentId::Claude),
        written.iter().cloned().collect()
    );
    let left: Vec<_> = fs::read_dir(store.dir())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, [ARCHIVE_FILE]);
    let _ = fs::remove_dir_all(home);
}

/// A scoped account's card and the legacy history it replaced, both labelled `a@example.com`.
fn scoped_with_history(store: &SnapshotStore) {
    store
        .save(
            &snapshot(
                AgentId::Codex,
                "profile:a",
                "a@example.com",
                "2026-09-20T10:00:00Z",
            )
            .with_legacy_id("team"),
        )
        .unwrap();
    store
        .save(&snapshot(
            AgentId::Codex,
            "team",
            "a@example.com",
            "2026-09-10T10:00:00Z",
        ))
        .unwrap();
}

/// Forget unarchives every id whose snapshot it deletes, the legacy history it takes along
/// included, and leaves every other archived id alone.
#[test]
fn forgetting_an_account_unarchives_every_id_it_deletes() {
    let home = scratch_dir("limits-archive-forget");
    let store = SnapshotStore::for_home(&home);
    scoped_with_history(&store);
    store
        .set_archived(
            AgentId::Codex,
            &ids(&["team", "profile:a", "profile:other", "legacy-b"]),
            true,
        )
        .unwrap();

    store.forget(AgentId::Codex, "profile:a").unwrap();
    assert_eq!(
        store.archived(AgentId::Codex),
        set(&["profile:other", "legacy-b"])
    );

    store
        .forget_matching_email(AgentId::Codex, "legacy-b", "b@example.com")
        .unwrap();
    assert_eq!(store.archived(AgentId::Codex), set(&["profile:other"]));

    // A profile with no snapshot of its own is forgotten by its id all the same.
    store.forget(AgentId::Codex, "profile:other").unwrap();
    assert!(store.archived(AgentId::Codex).is_empty());
    let _ = fs::remove_dir_all(home);
}

fn account(id: &str, legacy_id: Option<&str>, label: Option<&str>) -> LimitsAccountDto {
    LimitsAccountDto {
        id: id.to_string(),
        legacy_id: legacy_id.map(str::to_string),
        label: label.map(str::to_string),
    }
}

/// An unarchived account takes its own id out of the archive, and the legacy id its history was
/// kept under only while that history names the same email: in a shared workspace the legacy id
/// can hold another member's history, which stays archived.
#[test]
fn unarchiving_an_account_takes_along_only_its_own_legacy_history() {
    let home = scratch_dir("limits-archive-unarchive-account");
    let store = SnapshotStore::for_home(&home);
    scoped_with_history(&store);
    let archive = |values: &[&str]| {
        store
            .set_archived(AgentId::Codex, &ids(values), true)
            .unwrap();
    };

    archive(&["profile:a", "team", "profile:other"]);
    assert_eq!(
        store.unarchive_account(
            AgentId::Codex,
            &account("profile:a", Some("team"), Some(" A@Example.com "))
        ),
        Ok(true)
    );
    assert_eq!(store.archived(AgentId::Codex), set(&["profile:other"]));
    assert_eq!(
        store.unarchive_account(
            AgentId::Codex,
            &account("profile:a", Some("team"), Some("a@example.com"))
        ),
        Ok(false),
        "nothing left to unarchive"
    );

    archive(&["profile:b", "team"]);
    assert_eq!(
        store.unarchive_account(
            AgentId::Codex,
            &account("profile:b", Some("team"), Some("b@example.com"))
        ),
        Ok(true)
    );
    assert_eq!(
        store.archived(AgentId::Codex),
        set(&["profile:other", "team"]),
        "another member's history stays archived"
    );
    store
        .unarchive_account(AgentId::Codex, &account("profile:a", Some("team"), None))
        .unwrap();
    assert!(
        store.archived(AgentId::Codex).contains("team"),
        "without a label nothing ties the history to the account"
    );

    archive(&["legacy-without-file"]);
    store
        .unarchive_account(
            AgentId::Codex,
            &account(
                "profile:c",
                Some("legacy-without-file"),
                Some("c@example.com"),
            ),
        )
        .unwrap();
    assert!(
        store
            .archived(AgentId::Codex)
            .contains("legacy-without-file"),
        "no history names the account"
    );
    let _ = fs::remove_dir_all(home);
}
