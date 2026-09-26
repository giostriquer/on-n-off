//! The change protocol itself: which change kinds are refused and bump the sign-in epoch, what a
//! ticket guards, and when the lease is held.
use super::super::*;
use crate::dto::AgentId;
use serde_json::json;
use std::{path::Path, time::Duration};

const KEY: [u8; 32] = [6; 32];

fn open(home: &Path) -> Store {
    Store::open_with_key(home, true, |_, _| Ok(KEY)).unwrap()
}
fn loaded(home: &Path) -> Database {
    open(home).load().unwrap()
}
fn sealed(home: &Path) -> Vec<u8> {
    std::fs::read(home.join(".on-n-off/accounts/vault.enc")).unwrap()
}
fn journal() -> Recovery {
    Recovery {
        target_id: "switching".into(),
        outgoing: None,
        outgoing_identity: None,
    }
}
fn login(generation: &str) -> Login {
    Login {
        auth: json!({"tokens": {"refresh_token": generation}}),
        account: serde_json::Value::Null,
    }
}
/// A vault holding one Codex profile whose login is `generation`, owning its private renewal.
fn seeded(generation: &str) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let mut db = Database::default();
    let identity = Identity {
        provider: AgentId::Codex,
        user_id: "a".into(),
        workspace_id: "team".into(),
    };
    db.save(identity, login(generation), None).unwrap();
    db.profiles[0].usage_renewal_owned = true;
    open(home.path()).persist(&db).unwrap();
    home
}
fn interrupt(home: &Path) {
    let store = open(home);
    let mut db = store.load().unwrap();
    db.recovery = Some(journal());
    store.persist(&db).unwrap();
}
fn edit_category(db: &mut Database) -> Result<(), String> {
    db.profiles[0].category = Some("edited".into());
    Ok(())
}

/// One change kind's rule: whether a pending recovery refuses it, whether its absence does, and
/// whether it bumps the sign-in epoch.
struct Rule {
    name: &'static str,
    kind: fn() -> ChangeKind<'static>,
    refused_recovering: bool,
    refused_otherwise: bool,
    bumps: bool,
}

#[test]
fn each_change_kind_is_refused_and_bumps_the_epoch_by_its_rule() {
    let rule = |name, kind, refused_recovering, refused_otherwise, bumps| Rule {
        name,
        kind,
        refused_recovering,
        refused_otherwise,
        bumps,
    };
    let rules = [
        rule("account", || ChangeKind::Account, true, false, true),
        rule("recovery", || ChangeKind::Recovery, false, true, true),
        rule(
            "remembering",
            || ChangeKind::Remembering,
            false,
            false,
            true,
        ),
        rule("metadata", || ChangeKind::Metadata, false, false, false),
    ];
    for Rule {
        name,
        kind,
        refused_recovering,
        refused_otherwise,
        bumps,
    } in rules
    {
        for recovering in [false, true] {
            let home = seeded("a1");
            if recovering {
                interrupt(home.path());
            }
            let before = sealed(home.path());

            let result = open(home.path()).change(kind(), edit_category);

            let refused = if recovering {
                refused_recovering
            } else {
                refused_otherwise
            };
            assert_eq!(result.is_err(), refused, "{name}, recovering: {recovering}");
            if refused {
                assert_eq!(sealed(home.path()), before, "{name} wrote while refused");
                continue;
            }
            let after = loaded(home.path());
            assert_eq!(after.profiles[0].category.as_deref(), Some("edited"));
            assert_eq!(after.login_epoch, u64::from(bumps), "{name}");
            assert_eq!(
                after.recovery.is_some(),
                recovering,
                "{name} kept the journal"
            );
        }
    }
}

#[test]
fn an_edit_that_fails_writes_nothing() {
    let home = seeded("a1");
    let before = sealed(home.path());

    let result = open(home.path()).change(ChangeKind::Account, |db| {
        edit_category(db)?;
        Err::<(), _>("refused by the operation".to_string())
    });

    assert_eq!(result, Err("refused by the operation".into()));
    assert_eq!(sealed(home.path()), before);
}

#[test]
fn a_change_or_publication_that_changes_nothing_writes_nothing() {
    let home = seeded("a1");
    let before = sealed(home.path());
    let ticket = loaded(home.path()).ticket(Guard::SignIn).unwrap();

    open(home.path())
        .change(ChangeKind::Metadata, |_| Ok(()))
        .unwrap();
    open(home.path()).publish(&ticket, |_| Ok(())).unwrap();
    assert!(
        sealed(home.path()) == before,
        "an unchanged vault was rewritten"
    );

    open(home.path())
        .change(ChangeKind::Account, |_| Ok(()))
        .unwrap();
    assert!(sealed(home.path()) != before, "a bump is a change");
}

#[test]
fn a_change_is_durable_before_its_follow_up_and_released_before_it_returns() {
    let home = seeded("a1");

    let result =
        open(home.path()).change_then(ChangeKind::Account, edit_category, |(), db, persist| {
            let on_disk: Database = serde_json::from_slice(&super::super::super::vault::unseal(
                &KEY,
                &sealed(home.path()),
            )?)
            .unwrap();
            assert_eq!(on_disk.profiles[0].category.as_deref(), Some("edited"));
            assert_eq!(on_disk.login_epoch, 1);
            assert!(
                Store::lease_with_timeout(home.path(), Duration::ZERO).is_err(),
                "the follow-up runs under the lease"
            );
            db.profiles[0].category = Some("followed".into());
            persist(db)?;
            Err::<(), _>("the native half failed".to_string())
        });

    assert_eq!(
        result,
        Ok(Err("the native half failed".into())),
        "the change stood, and the follow-up's failure is its own"
    );
    assert!(Store::lease_with_timeout(home.path(), Duration::ZERO).is_ok());
    assert_eq!(
        loaded(home.path()).profiles[0].category.as_deref(),
        Some("followed")
    );
}

#[test]
fn a_sign_in_publishes_as_an_account_change_only_while_its_ticket_holds() {
    let home = seeded("a1");
    let ticket = loaded(home.path()).ticket(Guard::SignIn).unwrap();
    let recovering = seeded("b1");
    let late = loaded(recovering.path()).ticket(Guard::SignIn).unwrap();
    interrupt(recovering.path());

    open(home.path())
        .change(ChangeKind::SignIn(&ticket), edit_category)
        .unwrap();
    assert_eq!(
        loaded(home.path()).login_epoch,
        1,
        "a sign-in is an account change"
    );
    let before = sealed(home.path());
    assert!(open(home.path())
        .change(ChangeKind::SignIn(&ticket), edit_category)
        .is_err());
    assert_eq!(sealed(home.path()), before);
    assert!(open(recovering.path())
        .change(ChangeKind::SignIn(&late), edit_category)
        .is_err());
}

#[test]
fn a_publication_bumps_nothing_and_is_refused_once_its_ticket_no_longer_holds() {
    let home = seeded("a1");
    let ticket = loaded(home.path()).ticket(Guard::SignIn).unwrap();

    open(home.path()).publish(&ticket, edit_category).unwrap();
    open(home.path()).publish(&ticket, edit_category).unwrap();
    assert_eq!(loaded(home.path()).login_epoch, 0);

    open(home.path())
        .change(ChangeKind::Account, |_| Ok(()))
        .unwrap();
    let before = sealed(home.path());
    assert!(open(home.path()).publish(&ticket, edit_category).is_err());
    assert_eq!(sealed(home.path()), before);
}

#[test]
fn a_renewal_ticket_ignores_the_epoch_but_not_the_login_ownership_or_recovery() {
    let renewal = |home: &Path| {
        let db = loaded(home);
        db.ticket(Guard::Renewal(&db.profiles[0])).unwrap()
    };
    let publishes = |home: &Path, ticket: &Ticket| {
        open(home)
            .publish(ticket, |db| {
                db.profiles[0].login = Some(login("renewed"));
                Ok(())
            })
            .is_ok()
    };

    let home = seeded("a1");
    let ticket = renewal(home.path());
    open(home.path())
        .change(ChangeKind::Account, |_| Ok(()))
        .unwrap();
    assert!(
        publishes(home.path(), &ticket),
        "an unrelated account change"
    );

    let home = seeded("a1");
    let ticket = renewal(home.path());
    open(home.path())
        .change(ChangeKind::Metadata, |db| {
            db.profiles[0].login = Some(login("a2"));
            Ok(())
        })
        .unwrap();
    assert!(!publishes(home.path(), &ticket), "a new login generation");

    let home = seeded("a1");
    let ticket = renewal(home.path());
    open(home.path())
        .change(ChangeKind::Metadata, |db| {
            db.profiles[0].usage_renewal_owned = false;
            Ok(())
        })
        .unwrap();
    assert!(!publishes(home.path(), &ticket), "ownership revoked");
    let db = loaded(home.path());
    assert!(
        db.ticket(Guard::Renewal(&db.profiles[0])).is_err(),
        "a native shadow gets no renewal ticket"
    );

    let home = seeded("a1");
    let ticket = renewal(home.path());
    interrupt(home.path());
    assert!(!publishes(home.path(), &ticket), "a pending recovery");
}

#[test]
fn a_reading_ticket_holds_the_epoch_and_the_login_it_read_with() {
    let home = seeded("a1");
    let db = loaded(home.path());
    let reading = db
        .ticket(Guard::SignIn)
        .unwrap()
        .holding(&db.profiles[0], &login("a1"))
        .unwrap();
    assert!(open(home.path()).recheck(&reading).is_ok());

    let rotated = db.ticket(Guard::SignIn).unwrap();
    open(home.path())
        .publish(&rotated, |db| {
            db.profiles[0].login = Some(login("a2"));
            Ok(())
        })
        .unwrap();
    assert!(open(home.path()).recheck(&reading).is_err(), "a new login");
    let reading = rotated.holding(&db.profiles[0], &login("a2")).unwrap();
    assert!(open(home.path()).recheck(&reading).is_ok());

    open(home.path())
        .change(ChangeKind::Metadata, edit_category)
        .unwrap();
    assert!(
        open(home.path()).recheck(&reading).is_ok(),
        "a category edit"
    );
    open(home.path())
        .change(ChangeKind::Account, |_| Ok(()))
        .unwrap();
    assert!(
        open(home.path()).recheck(&reading).is_err(),
        "an account change"
    );
}

#[test]
fn a_gate_applies_the_rule_without_creating_or_writing_a_vault() {
    let home = tempfile::tempdir().unwrap();
    assert!(Store::gate(home.path(), &ChangeKind::Account).is_ok());
    assert_eq!(
        Store::gate(home.path(), &ChangeKind::Recovery),
        Err("No recovery is pending.".into())
    );
    assert!(!Store::vault_exists(home.path()));

    // The key `vault::tests::unlock_fixture` gives this home.
    let key = [7; 32];
    let store = Store::open_with_key(home.path(), true, |_, _| Ok(key)).unwrap();
    let db = Database {
        recovery: Some(journal()),
        ..Database::default()
    };
    store.persist(&db).unwrap();
    drop(store);
    super::super::super::vault::tests::unlock_fixture(&home);
    let before = sealed(home.path());

    assert_eq!(
        Store::gate(home.path(), &ChangeKind::Account),
        Err(RECOVER_FIRST.into())
    );
    assert!(Store::gate(home.path(), &ChangeKind::Recovery).is_ok());
    assert!(Store::gate(home.path(), &ChangeKind::Metadata).is_ok());
    assert_eq!(sealed(home.path()), before);
    assert!(
        Store::lease_with_timeout(home.path(), Duration::ZERO).is_ok(),
        "the gate releases the lease"
    );
}
