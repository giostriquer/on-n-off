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
