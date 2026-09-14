use super::*;
use serde_json::json;
#[test]
fn validates_helper_identity_before_accepting_billing() {
    let value =
        json!({"accountId":"account-a","active_until":"2026-10-10T12:00:00Z","will_renew":false});
    assert!(parse_reply(&value.to_string(), "account-a").is_ok());
    assert!(parse_reply(&value.to_string(), "account-b").is_err());
    assert!(parse_reply(
        r#"{"accountId":"account-a","active_until":"bad","will_renew":false}"#,
        "account-a"
    )
    .is_err());
}
#[test]
fn empty_billing_is_authoritative_and_failures_preserve_it() {
    let home = crate::subscription::tests::fixture_home();
    let mut state = State {
        generation: 7,
        active: Some("account-a".into()),
        ..Default::default()
    };
    let empty = parse_reply(
        r#"{"accountId":"account-a","active_until":null,"will_renew":false}"#,
        "account-a",
    )
    .unwrap();
    assert!(publish(&home, &mut state, 7, "account-a", Ok(empty)).is_ok());
    assert_eq!(store::load(&home, "account-a").unwrap().date, None);
    state.active = Some("account-a".into());
    assert!(publish(&home, &mut state, 7, "account-a", Err("Unavailable".into())).is_err());
    assert_eq!(store::load(&home, "account-a").unwrap().date, None);
    std::fs::remove_dir_all(home).unwrap();
}
#[test]
fn disconnect_and_account_switch_refuse_late_results() {
    let home = crate::subscription::tests::fixture_home();
    let result = || {
        parse_reply(
            r#"{"accountId":"account-a","active_until":"2026-10-10T12:00:00Z","will_renew":true}"#,
            "account-a",
        )
    };
    let mut state = State {
        generation: 8,
        active: None,
        unavailable: Default::default(),
        ..Default::default()
    };
    assert!(publish(&home, &mut state, 7, "account-a", result()).is_err());
    state.active = Some("account-a".into());
    std::fs::write(home.join(".codex/auth.json"), "{}").unwrap();
    assert!(publish(&home, &mut state, 8, "account-a", result()).is_err());
    assert!(store::load(&home, "account-a").is_none());
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn automatic_import_reserves_once_and_refuses_a_changed_local_account() {
    let home = crate::subscription::tests::fixture_home();
    let now = "2026-09-10T12:00:00Z"
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();
    let metadata =
        super::super::parse_billing(&json!({"active_until":null,"will_renew":false}), now).unwrap();
    store::save(&home, "account-a", &metadata).unwrap();
    let mut state = State::default();
    let due = now + chrono::Duration::days(1);
    assert_eq!(reserve_auto(&home, &mut state, "account-a", due), Some(1));
    assert_eq!(reserve_auto(&home, &mut state, "account-a", due), None);
    assert!(publish(&home, &mut state, 1, "account-a", Err("offline".into())).is_err());
    assert_eq!(reserve_auto(&home, &mut state, "account-a", due), None);
    assert_eq!(store::load(&home, "account-a").unwrap(), metadata);
    std::fs::write(home.join(".codex/auth.json"), "{}").unwrap();
    assert_eq!(
        reserve_auto(
            &home,
            &mut state,
            "account-a",
            due + chrono::Duration::days(1)
        ),
        None
    );
    std::fs::remove_dir_all(home).unwrap();
}

fn publish(
    home: &Path,
    state: &mut State,
    generation: u64,
    account: &str,
    result: Result<SubscriptionDate, String>,
) -> Result<(), String> {
    super::super::with_billing_context(home, account, |context| {
        super::publish(
            home,
            state,
            generation,
            account,
            result,
            super::super::validate_context(context),
        )
    })
}
fn reserve_auto(
    home: &Path,
    state: &mut State,
    account: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<u64> {
    let context = super::super::billing_context(home, account).unwrap();
    super::reserve_auto(home, state, account, now, context.as_ref())
}
#[test]
fn an_unsaved_native_login_does_not_start_browser_access_without_prior_billing() {
    let home = crate::subscription::tests::fixture_home();
    assert!(super::super::validate_account(&home, "account-a").is_ok());
    assert_eq!(
        reserve_auto(
            &home,
            &mut State::default(),
            "account-a",
            chrono::Utc::now()
        ),
        None
    );
    assert!(!home.join(".on-n-off/subscriptions").exists());
}
#[test]
fn inactive_saved_account_can_reserve_its_first_billing_check_without_switching_native_login() {
    let home = crate::subscription::tests::fixture_home();
    let before = std::fs::read(home.join(".codex/auth.json")).unwrap();
    let identity = crate::accounts::model::Identity {
        provider: crate::dto::AgentId::Codex,
        user_id: "saved-user".into(),
        workspace_id: "saved-workspace".into(),
    };
    let key = identity.observation_key();
    let context = super::super::resolve_context(&home, &key, Some(identity)).unwrap();
    assert_eq!(context.workspace, "saved-workspace");
    assert_eq!(context.user, "saved-user");
    let now = chrono::Utc::now();
    assert_eq!(
        super::reserve_auto(&home, &mut State::default(), &key, now, Some(&context)),
        Some(1)
    );
    // A new process still observes the persisted attempt despite having no billing response.
    assert_eq!(
        super::reserve_auto(&home, &mut State::default(), &key, now, Some(&context)),
        None
    );
    assert!(store::load(&home, &key).is_none());
    assert_eq!(
        std::fs::read(home.join(".codex/auth.json")).unwrap(),
        before
    );
}

#[test]
fn manual_billing_waits_for_the_running_import_then_reserves_its_own_account() {
    let imports = Imports::default();
    imports.reserve_manual("account-a", Duration::ZERO).unwrap();
    std::thread::scope(|threads| {
        let pending = threads.spawn(|| imports.reserve_manual("account-b", Duration::from_secs(2)));
        // A running check finishes asynchronously, while the other card is waiting.
        wait_for_manual_request(&imports);
        let mut lock = imports.state.lock().unwrap();
        assert_eq!(lock.as_ref().unwrap().active.as_deref(), Some("account-a"));
        lock.as_mut().unwrap().active = None;
        drop(lock);
        imports.available.notify_all();
        assert_eq!(pending.join().unwrap(), Ok(2));
    });
    assert_eq!(
        imports
            .state
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .active
            .as_deref(),
        Some("account-b")
    );
}

#[test]
fn queued_manual_request_times_out_without_replacing_the_active_import() {
    let imports = Imports::default();
    imports.reserve_manual("account-a", Duration::ZERO).unwrap();
    assert!(imports
        .reserve_manual("account-b", Duration::from_millis(5))
        .is_err());
    let lock = imports.state.lock().unwrap();
    let state = lock.as_ref().unwrap();
    assert_eq!(state.active.as_deref(), Some("account-a"));
    assert_eq!(state.waiting_manual, 0);
}

#[test]
fn account_invalidation_cancels_a_waiting_manual_check() {
    let imports = Imports::default();
    imports.reserve_manual("account-a", Duration::ZERO).unwrap();
    std::thread::scope(|threads| {
        let pending = threads.spawn(|| imports.reserve_manual("account-b", Duration::from_secs(2)));
        wait_for_manual_request(&imports);
        let mut lock = imports.state.lock().unwrap();
        let state = lock.as_mut().unwrap();
        state.cancellation += 1;
        state.active = None;
        drop(lock);
        imports.available.notify_all();
        assert!(pending.join().unwrap().is_err());
    });
    assert!(imports
        .state
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .active
        .is_none());
}

fn wait_for_manual_request(imports: &Imports) {
    let lock = imports.state.lock().unwrap();
    let (lock, _) = imports
        .available
        .wait_timeout_while(lock, Duration::from_secs(2), |state| {
            state.as_ref().is_none_or(|state| state.waiting_manual == 0)
        })
        .unwrap();
    assert_eq!(
        lock.as_ref().unwrap().waiting_manual,
        1,
        "manual request did not enter the queue"
    );
}

#[test]
fn automatic_reads_leave_the_next_slot_for_a_waiting_manual_request() {
    let home = crate::subscription::tests::fixture_home();
    let now = "2026-09-10T12:00:00Z"
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();
    let metadata =
        super::super::parse_billing(&json!({"active_until":null,"will_renew":false}), now).unwrap();
    store::save(&home, "account-a", &metadata).unwrap();
    let due = now + chrono::Duration::days(1);
    let mut state = State {
        waiting_manual: 1,
        ..Default::default()
    };
    assert_eq!(reserve_auto(&home, &mut state, "account-a", due), None);
    state.waiting_manual = 0;
    // A skipped automatic request must not spend its daily attempt.
    assert_eq!(reserve_auto(&home, &mut state, "account-a", due), Some(1));
    std::fs::remove_dir_all(home).unwrap();
}
