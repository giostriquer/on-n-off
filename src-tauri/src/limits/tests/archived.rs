use super::account;
use crate::dto::LimitWindowKind;
use crate::limits::json::window;
use crate::limits::*;
use crate::paths::scratch_dir;

fn read(id: &str, label: &str, hour: u32) -> ProviderLimitsDto {
    let mut dto = finish(
        AgentId::Codex,
        LimitsStatus::Ok,
        None,
        Parsed {
            account: Some(account(id, label)),
            reading: Reading {
                windows: vec![window(
                    "primary",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    40.0,
                    None,
                )],
                ..Reading::default()
            },
        },
    );
    for window in &mut dto.reading.windows {
        window.observed_at = format!("2026-09-20T{hour:02}:00:00.000Z");
    }
    dto
}

fn archive(store: &SnapshotStore, ids: &[&str]) {
    let ids: Vec<String> = ids.iter().map(|id| (*id).to_string()).collect();
    store.set_archived(AgentId::Codex, &ids, true).unwrap();
}

fn summary(listed: &[ProviderLimitsDto]) -> Vec<(&str, bool, bool)> {
    listed
        .iter()
        .map(|dto| {
            (
                dto.account.as_ref().unwrap().id.as_str(),
                dto.current_account,
                dto.archived,
            )
        })
        .collect()
}

#[test]
fn archived_accounts_are_flagged_and_still_hide_the_history_they_replaced() {
    let home = scratch_dir("limits-archived-flag");
    let store = SnapshotStore::for_home(&home);
    let mut scoped = read("profile:a", "a@example.com", 10).with_legacy_id("team");
    scoped.current_account = false;
    store.save(&scoped).unwrap();
    store.save(&read("team", "a@example.com", 8)).unwrap();
    store.save(&read("acct-b", "b@example.com", 9)).unwrap();
    archive(&store, &["profile:a", "team"]);

    let mut listed = aggregate_accounts(&store, read("acct-me", "me@example.com", 11));
    flag_archived_at(&home, AgentId::Codex, &mut listed);

    assert_eq!(
        summary(&listed),
        [
            ("acct-me", true, false),
            ("profile:a", false, true),
            ("acct-b", false, false)
        ],
        "the archived legacy history stays hidden behind its archived card"
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn the_signed_in_card_is_never_flagged_archived() {
    let home = scratch_dir("limits-archived-signed-in");
    let store = SnapshotStore::for_home(&home);
    store.save(&read("acct-b", "b@example.com", 9)).unwrap();
    archive(&store, &["acct-me", "acct-b"]);

    let mut listed = aggregate_accounts(&store, read("acct-me", "me@example.com", 11));
    flag_archived_at(&home, AgentId::Codex, &mut listed);

    assert_eq!(
        summary(&listed),
        [("acct-me", true, false), ("acct-b", false, true)]
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn the_signed_in_account_a_read_names_is_unarchived() {
    let home = scratch_dir("limits-archived-unarchive-signed-in");
    let store = SnapshotStore::for_home(&home);
    store.save(&read("team", "a@example.com", 8)).unwrap();
    archive(&store, &["profile:a", "team", "acct-b"]);
    let signed_in = read("profile:a", "a@example.com", 11).with_legacy_id("team");
    let mut remembered = read("acct-b", "b@example.com", 9);
    remembered.current_account = false;
    remembered.archived = true;

    assert!(unarchive_signed_in_at(
        &home,
        AgentId::Codex,
        &[signed_in, remembered.clone()]
    ));
    assert_eq!(
        store.archived(AgentId::Codex),
        ["acct-b".to_string()].into()
    );

    let signed_out = finish(
        AgentId::Codex,
        LimitsStatus::SignedOut,
        None,
        Parsed::default(),
    );
    assert!(!unarchive_signed_in_at(
        &home,
        AgentId::Codex,
        &[signed_out, remembered]
    ));
    assert_eq!(
        store.archived(AgentId::Codex),
        ["acct-b".to_string()].into()
    );
    let _ = std::fs::remove_dir_all(home);
}

#[cfg(unix)]
#[test]
fn a_signed_in_read_that_cannot_write_the_archive_says_nothing_changed() {
    use std::os::unix::fs::PermissionsExt;
    let home = scratch_dir("limits-archived-unwritable");
    let store = SnapshotStore::for_home(&home);
    archive(&store, &["acct-me"]);
    let dir = home.join(".on-n-off").join("limits");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let changed = unarchive_signed_in_at(
        &home,
        AgentId::Codex,
        &[read("acct-me", "me@example.com", 11)],
    );

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!changed);
    assert_eq!(
        store.archived(AgentId::Codex),
        ["acct-me".to_string()].into()
    );
    let _ = std::fs::remove_dir_all(home);
}
