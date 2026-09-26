use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::json;

/// An unsigned ID token whose `https://api.openai.com/auth` claims are `claims`, shaped the way
/// `claims()` decodes it: fixtures across the crate build their logins from it.
pub(crate) fn id_token(claims: &Value) -> String {
    let payload = json!({"https://api.openai.com/auth": claims});
    format!(
        "header.{}.signature",
        URL_SAFE_NO_PAD.encode(payload.to_string())
    )
}
fn auth(user: &str, workspace: &str) -> Value {
    let claims = json!({"sub":user,"https://api.openai.com/auth":{"chatgpt_user_id":user,"chatgpt_account_id":workspace}});
    json!({"tokens":{"id_token":format!("e30.{}.sig",URL_SAFE_NO_PAD.encode(claims.to_string())),"access_token":"access","refresh_token":"renewable","account_id":workspace}})
}
#[test]
fn scopes_profiles_to_both_user_and_workspace() {
    let a = identity(AgentId::Codex, &auth("user-a", "team"), &Value::Null).unwrap();
    let b = identity(AgentId::Codex, &auth("user-b", "team"), &Value::Null).unwrap();
    let personal = identity(AgentId::Codex, &auth("user-a", "personal"), &Value::Null).unwrap();
    assert_ne!(a, b);
    assert_ne!(a, personal);
    assert_eq!(a.user_id, "user-a");
    assert_eq!(a.workspace_id, "team");
}
#[test]
fn refuses_codex_workspace_claim_disagreement_and_access_only_login() {
    let mut value = auth("user-a", "team");
    value["tokens"]["account_id"] = json!("other");
    assert!(identity(AgentId::Codex, &value, &Value::Null).is_err());
    let mut value = auth("user-a", "team");
    value["tokens"]
        .as_object_mut()
        .unwrap()
        .remove("refresh_token");
    assert!(identity(AgentId::Codex, &value, &Value::Null).is_err());
}
#[test]
fn claude_same_user_different_organizations_are_distinct() {
    let credential = json!({"claudeAiOauth":{"accessToken":"access","refreshToken":"refresh"}});
    let a = identity(
        AgentId::Claude,
        &credential,
        &json!({"accountUuid":"a","organizationUuid":"org-a"}),
    )
    .unwrap();
    let b = identity(
        AgentId::Claude,
        &credential,
        &json!({"accountUuid":"a","organizationUuid":"org-b"}),
    )
    .unwrap();
    assert_ne!(a, b);
    assert!(identity(
        AgentId::Claude,
        &credential,
        &json!({"emailAddress":"same@example.com"})
    )
    .is_err());
}
#[test]
fn a_codex_login_renews_soon_within_ten_minutes_of_expiry_or_without_a_readable_one() {
    let expiring_at = |exp: i64| {
        let mut value = auth("user-a", "team");
        let claims = json!({ "exp": exp });
        value["tokens"]["access_token"] = json!(format!(
            "e30.{}.sig",
            URL_SAFE_NO_PAD.encode(claims.to_string())
        ));
        value
    };
    assert!(!codex_renews_soon(&expiring_at(1_000_600), 1_000_000));
    assert!(codex_renews_soon(&expiring_at(1_000_599), 1_000_000));
    assert!(codex_renews_soon(&auth("user-a", "team"), 1_000_000));
}

/// A Claude profile is the account in its organization, and only a renewable login is one: both
/// tokens are required, and blank ones count as missing.
#[test]
fn a_claude_identity_is_the_account_in_its_organization_and_needs_both_tokens() {
    let account =
        json!({"accountUuid":"user-a","organizationUuid":"org-a","emailAddress":"a@example.com"});
    let renewable = json!({"claudeAiOauth":{"accessToken":"access","refreshToken":"refresh"}});
    let found = identity(AgentId::Claude, &renewable, &account).unwrap();
    assert_eq!(
        (
            found.provider,
            found.user_id.as_str(),
            found.workspace_id.as_str()
        ),
        (AgentId::Claude, "user-a", "org-a")
    );
    for auth in [
        json!({"claudeAiOauth":{"accessToken":"access"}}),
        json!({"claudeAiOauth":{"refreshToken":"refresh"}}),
        json!({"claudeAiOauth":{"accessToken":" ","refreshToken":"refresh"}}),
    ] {
        assert!(
            identity(AgentId::Claude, &auth, &account).is_err(),
            "{auth}"
        );
    }
}

/// A Codex login that carries an API key is not a subscription, so it is never a profile; a key
/// left null is no key.
#[test]
fn a_codex_api_key_login_is_not_a_subscription_profile() {
    let mut value = auth("user-a", "team");
    value["OPENAI_API_KEY"] = json!("fixture-api-key");
    assert_eq!(
        identity(AgentId::Codex, &value, &Value::Null)
            .err()
            .as_deref(),
        Some("API key logins cannot be saved as subscription profiles.")
    );
    value["OPENAI_API_KEY"] = Value::Null;
    assert!(identity(AgentId::Codex, &value, &Value::Null).is_ok());
}
