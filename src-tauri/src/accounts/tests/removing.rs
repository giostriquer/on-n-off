use super::fixture::{claude, codex, identity, Harness};
use crate::accounts::Forgot;
use crate::dto::AgentId;

fn named(harness: &Harness, provider: AgentId) -> Vec<(String, String)> {
    harness
        .vault()
        .categories()
        .get(provider.key())
        .into_iter()
        .flatten()
        .map(|(id, category)| (id.clone(), category.clone()))
        .collect()
}

#[test]
fn forgetting_a_saved_card_removes_its_profile_and_the_categories_of_every_id_it_holds_in_one_change(
) {
    let harness = Harness::new();
    let a = identity(AgentId::Codex, "a", "team");
    let b = identity(AgentId::Codex, "b", "team");
    let kept = harness.saved(b.clone(), codex("b", "b1"));
    harness.saved(a.clone(), codex("a", "a1"));
    harness.seed(|db| {
        for (id, category) in [
            (a.observation_key(), "Client A"),
            ("team".to_string(), "Old team"),
            (b.observation_key(), "Client B"),
        ] {
            db.set_account_category(AgentId::Codex, &id, category)
                .unwrap();
        }
    });
    let sign_in = harness.sign_in();

    let forgot = harness.accounts().forget_account(
        AgentId::Codex,
        &a.observation_key(),
        &["team".to_string(), a.observation_key()],
    );

    assert_eq!(
        forgot,
        Ok(Forgot {
            profile: true,
            changed: true,
            home_left: None,
        })
    );
    let vault = harness.vault();
    let left: Vec<_> = vault.profiles.iter().map(|p| p.id.clone()).collect();
    assert_eq!(left, [kept]);
    assert_eq!(vault.ignored_accounts, [a], "kept out of remembering");
    assert_eq!(
        named(&harness, AgentId::Codex),
        [(b.observation_key(), "Client B".to_string())]
    );
    assert!(
        !harness.vouches(&sign_in),
        "removing a saved login is an account change"
    );
}

#[test]
fn forgetting_a_history_card_drops_only_its_categories_and_leaves_sign_ins_running() {
    let harness = Harness::new();
    let b = identity(AgentId::Codex, "b", "team");
    harness.saved(b.clone(), codex("b", "b1"));
    harness.seed(|db| {
        db.set_account_category(AgentId::Codex, "team", "Old team")
            .unwrap();
    });
    let sign_in = harness.sign_in();
    let forget = || {
        harness
            .accounts()
            .forget_account(AgentId::Codex, "team", &["team".into()])
    };

    assert_eq!(
        forget(),
        Ok(Forgot {
            profile: false,
            changed: true,
            home_left: None,
        })
    );
    assert_eq!(harness.vault().profiles.len(), 1);
    assert!(named(&harness, AgentId::Codex).is_empty());
    assert!(
        harness.vouches(&sign_in),
        "a category is metadata, so a sign-in in flight still publishes"
    );
    let sealed = harness.sealed();
    assert_eq!(forget(), Ok(Forgot::default()));
    assert_eq!(
        harness.sealed(),
        sealed,
        "nothing left to drop writes nothing"
    );
}

#[test]
fn a_pending_recovery_refuses_to_remove_a_saved_login_and_writes_nothing() {
    let harness = Harness::new();
    let a = identity(AgentId::Codex, "a", "team");
    let id = harness.saved(a.clone(), codex("a", "a1"));
    harness.interrupted(&id, None);
    let sealed = harness.sealed();

    let refused = harness.accounts().forget_account(
        AgentId::Codex,
        &a.observation_key(),
        &[a.observation_key()],
    );

    assert_eq!(
        refused,
        Err("Recover the interrupted account change first.".to_string())
    );
    assert_eq!(harness.sealed(), sealed);
    assert_eq!(harness.vault().profiles.len(), 1);
}

#[test]
fn forgetting_with_no_vault_creates_none() {
    let harness = Harness::new();

    assert_eq!(
        harness
            .accounts()
            .forget_account(AgentId::Claude, "user-a", &["user-a".into()]),
        Ok(Forgot::default())
    );
    assert!(harness.sealed().is_none());
}

#[test]
fn a_saved_login_of_another_provider_is_left_alone() {
    let harness = Harness::new();
    let a = identity(AgentId::Codex, "a", "team");
    harness.saved(a.clone(), codex("a", "a1"));

    let forgot = harness.accounts().forget_account(
        AgentId::Claude,
        &a.observation_key(),
        &[a.observation_key()],
    );

    assert_eq!(forgot, Ok(Forgot::default()));
    assert_eq!(harness.vault().profiles.len(), 1);
}

fn homed(harness: &Harness, user: &str) -> (String, std::path::PathBuf) {
    let account = identity(AgentId::Claude, user, "team");
    let id = harness.saved(account.clone(), claude(user, "1"));
    let home = uuid::Uuid::new_v4().to_string();
    harness.seed(|db| {
        let profile = db.profiles.iter_mut().find(|p| p.id == id).unwrap();
        profile.home = Some(home.clone());
        profile.login = None;
    });
    let dir = super::super::homes::dir(harness.path(), &home).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    harness
        .homes()
        .logins
        .lock()
        .unwrap()
        .insert(dir.clone(), claude(user, "1"));
    *harness.homes().sealed.lock().unwrap() = true;
    (account.observation_key(), dir)
}

#[test]
fn forgetting_a_saved_card_retires_the_home_its_profile_named_within_the_call() {
    let harness = Harness::new().with_homes();
    let (key, dir) = homed(&harness, "a");
    let (_, kept) = homed(&harness, "b");

    let forgot =
        harness
            .accounts()
            .forget_account(AgentId::Claude, &key, std::slice::from_ref(&key));

    assert_eq!(
        forgot,
        Ok(Forgot {
            profile: true,
            changed: true,
            home_left: None,
        })
    );
    assert_eq!(
        *harness.homes().deleted.lock().unwrap(),
        std::slice::from_ref(&dir),
        "by its own provider's home, which deletes the login with it"
    );
    assert!(!dir.exists());
    assert!(kept.exists(), "another account's home stays");
}

#[test]
fn a_home_that_cannot_go_now_is_left_for_a_later_read_and_says_why() {
    for (cause, why) in [
        ("busy", "Claude is busy with this home."),
        ("undeletable", "The home could not be removed."),
    ] {
        let harness = Harness::new().with_homes();
        let (key, dir) = homed(&harness, "a");
        match cause {
            "busy" => harness.homes().busy.lock().unwrap().push(dir.clone()),
            _ => harness
                .homes()
                .undeletable
                .lock()
                .unwrap()
                .push(dir.clone()),
        }

        let forgot =
            harness
                .accounts()
                .forget_account(AgentId::Claude, &key, std::slice::from_ref(&key));

        assert_eq!(
            forgot,
            Ok(Forgot {
                profile: true,
                changed: true,
                home_left: Some(why.to_string()),
            }),
            "{cause}"
        );
        assert!(dir.exists(), "{cause}: left for a later read");
        assert!(harness.vault().profiles.is_empty(), "{cause}");
    }
}

#[test]
fn forgetting_a_saved_card_ends_a_sign_in_running_for_its_profile_at_once() {
    let harness = Harness::new();
    let account = identity(AgentId::Claude, "b", "team");
    let id = harness.saved(account.clone(), claude("b", "b1"));
    *harness.native.signed_in.borrow_mut() = Some(claude("b", "b2"));
    harness.native.sign_in_waits.set(true);
    let home = harness.path().to_path_buf();
    let remover = std::thread::spawn(move || {
        let logins = home.join(".on-n-off/accounts/logins");
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(20) {
            let running = std::fs::read_dir(&logins)
                .into_iter()
                .flatten()
                .flatten()
                .any(|entry| entry.path().join("bin").exists());
            if running {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let key = account.observation_key();
        crate::accounts::forget_account_at(&home, AgentId::Claude, &key, std::slice::from_ref(&key))
    });
    let started = std::time::Instant::now();

    let error = harness
        .accounts()
        .add(AgentId::Claude, uuid::Uuid::new_v4().to_string(), Some(id))
        .unwrap_err();

    assert_eq!(
        remover.join().unwrap().map(|forgot| forgot.profile),
        Ok(true)
    );
    assert!(
        error.starts_with("Sign-in was canceled, timed out, or did not complete."),
        "{error}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "it ended when the profile went, not when the sign-in gave up"
    );
}
