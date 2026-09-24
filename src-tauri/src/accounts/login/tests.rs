use super::*;
#[test]
fn cancellation_before_worker_start_prevents_a_late_login() {
    let mut registry = Registry::default();
    registry.cancel("operation-a");
    assert!(registry.reserve("operation-a", None).is_err());
    assert!(registry.active.is_none());
}
#[test]
fn cancellation_is_scoped_and_old_operations_cannot_publish() {
    let mut registry = Registry::default();
    let flag = registry.reserve("a", Some("profile-a".into())).unwrap();
    registry.cancel("b");
    assert!(!flag.load(Ordering::Acquire));
    assert!(registry.current("a"));
    registry.cancel("a");
    assert!(flag.load(Ordering::Acquire));
    assert!(!registry.current("a"));
    registry.active = None;
    registry.reserve("c", None).unwrap();
    assert!(!registry.current("a"));
    assert!(registry.current("c"));
}

use super::super::store::Login;
use crate::dto::{
    LimitWindowDto, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto,
};
use serde_json::json;
use std::cell::RefCell;

struct IsolatedLogin(RefCell<Login>);
impl Native for IsolatedLogin {
    fn read(&self) -> Result<Option<Login>, String> {
        Ok(Some(self.0.borrow().clone()))
    }
    fn identify(&self, login: &Login) -> Result<Identity, String> {
        super::super::model::identity(AgentId::Codex, &login.auth, &login.account)
    }
    fn verify(&self) -> Result<(), String> {
        Ok(())
    }
    fn write(&self, _: Option<&Login>) -> Result<(), String> {
        panic!("must not activate a login")
    }
}
fn fixture_login(user: &str, workspace: &str, generation: &str) -> Login {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let claims = json!({"email":"same@example.com", "https://api.openai.com/auth":{
        "chatgpt_user_id":user,"chatgpt_account_id":workspace}});
    Login {
        auth: json!({"tokens":{"account_id":workspace,"access_token":generation,
        "refresh_token":generation,"id_token":format!("e30.{}.s", URL_SAFE_NO_PAD.encode(claims.to_string()))}}),
        account: serde_json::Value::Null,
    }
}
fn usage(identity: &Identity) -> ProviderLimitsDto {
    ProviderLimitsDto {
        provider: AgentId::Codex,
        status: LimitsStatus::Ok,
        message: None,
        account: Some(LimitsAccountDto {
            id: identity.observation_key(),
            legacy_id: Some(identity.workspace_id.clone()),
            label: Some("same@example.com".into()),
        }),
        current_account: false,
        plan: Some("pro".into()),
        credits: None,
        workspace_credits: None,
        reset_credits: None,
        reset_offer: None,
        windows: vec![LimitWindowDto {
            id: "primary".into(),
            label: "Weekly · all models".into(),
            kind: LimitWindowKind::Weekly,
            used_percent: 42.0,
            window_seconds: Some(604800),
            resets_at: None,
            observed_at: "2026-09-13T12:00:00Z".into(),
        }],
    }
}
#[test]
fn sign_in_keeps_usage_with_the_new_profile_and_saves_the_latest_cli_generation() {
    let isolated = IsolatedLogin(RefCell::new(fixture_login("user", "team", "first")));
    let prepared = prepare_login(&isolated, |_, identity| {
        *isolated.0.borrow_mut() = fixture_login("user", "team", "rotated-by-cli");
        Some(usage(identity))
    })
    .unwrap();
    let snapshot = prepared
        .usage
        .expect("newly signed-in account must carry its usage");
    assert_eq!(snapshot.windows[0].used_percent, 42.0);
    assert_eq!(snapshot.plan.as_deref(), Some("pro"));
    assert!(!snapshot.current_account);
    assert_eq!(
        prepared.login.auth["tokens"]["refresh_token"],
        "rotated-by-cli"
    );
}
#[test]
fn identity_change_during_usage_read_rejects_the_entire_sign_in() {
    for (user, workspace) in [("other-user", "team"), ("user", "other-team")] {
        let isolated = IsolatedLogin(RefCell::new(fixture_login("user", "team", "first")));
        assert!(prepare_login(&isolated, |_, identity| {
            *isolated.0.borrow_mut() = fixture_login(user, workspace, "new");
            Some(usage(identity))
        })
        .is_err());
    }
}
#[test]
fn failed_usage_read_still_keeps_the_successful_login() {
    let isolated = IsolatedLogin(RefCell::new(fixture_login("user", "team", "first")));
    let prepared = prepare_login(&isolated, |_, _| None).unwrap();
    assert!(prepared.usage.is_none());
    assert_eq!(prepared.login.auth["tokens"]["access_token"], "first");
}
#[test]
fn usage_from_another_identity_cannot_attach_by_matching_email() {
    let isolated = IsolatedLogin(RefCell::new(fixture_login("user", "team", "first")));
    let prepared = prepare_login(&isolated, |_, identity| {
        let mut other = identity.clone();
        other.user_id = "other-user".into();
        Some(usage(&other))
    })
    .unwrap();
    assert!(prepared.usage.is_none());
}
