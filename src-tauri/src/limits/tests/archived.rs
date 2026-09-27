//! Archived accounts in a provider's list: flagged once the legacy history their cards replaced is
//! hidden, so that history stays hidden behind them, and the signed-in card never.

use super::account;
use crate::dto::LimitWindowKind;
use crate::limits::json::window;
use crate::limits::*;
use crate::paths::scratch_dir;

/// A successful read of `id`, labelled `label`, observed at `hour` o'clock.
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

/// What each listed card is: its id, whether it is signed in and whether it is archived.
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

    let listed = aggregate_accounts(&store, read("acct-me", "me@example.com", 11));

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

/// Being signed in unarchives an account, so its card is never flagged, even while the archive
/// still names it.
#[test]
fn the_signed_in_card_is_never_flagged_archived() {
    let home = scratch_dir("limits-archived-signed-in");
    let store = SnapshotStore::for_home(&home);
    store.save(&read("acct-b", "b@example.com", 9)).unwrap();
    archive(&store, &["acct-me", "acct-b"]);

    let listed = aggregate_accounts(&store, read("acct-me", "me@example.com", 11));

    assert_eq!(
        summary(&listed),
        [("acct-me", true, false), ("acct-b", false, true)]
    );
    let _ = std::fs::remove_dir_all(home);
}
