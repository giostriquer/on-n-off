use super::*;
use serde_json::json;

/// A Claude login with these credentials and this account record.
fn login(auth: Value, account: Value) -> Login {
    Login { auth, account }
}

fn identity_of(auth: Value, account: Value) -> Result<Identity, String> {
    ClaudeLogin::of(&login(auth, account)).identity()
}

#[test]
fn claude_same_user_different_organizations_are_distinct() {
    let credential = json!({"claudeAiOauth":{"accessToken":"access","refreshToken":"refresh"}});
    let a = identity_of(
        credential.clone(),
        json!({"accountUuid":"a","organizationUuid":"org-a"}),
    )
    .unwrap();
    let b = identity_of(
        credential.clone(),
        json!({"accountUuid":"a","organizationUuid":"org-b"}),
    )
    .unwrap();
    assert_ne!(a, b);
    assert!(identity_of(credential, json!({"emailAddress":"same@example.com"})).is_err());
}

/// A Claude profile is the account in its organization, and only a renewable login is one: both
/// tokens are required, and blank ones count as missing.
#[test]
fn a_claude_identity_is_the_account_in_its_organization_and_needs_both_tokens() {
    let account =
        json!({"accountUuid":"user-a","organizationUuid":"org-a","emailAddress":"a@example.com"});
    let renewable = json!({"claudeAiOauth":{"accessToken":"access","refreshToken":"refresh"}});
    let found = identity_of(renewable, account.clone()).unwrap();
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
            identity_of(auth.clone(), account.clone()).is_err(),
            "{auth}"
        );
    }
}

/// A Claude login's email is its account record's, trimmed; a blank or missing one is none.
#[test]
fn a_claude_logins_email_is_its_account_records() {
    let email = |account: Value| {
        ClaudeLogin::of(&login(
            json!({"claudeAiOauth":{"accessToken":"access","refreshToken":"refresh"}}),
            account,
        ))
        .email()
    };
    assert_eq!(
        email(json!({"emailAddress":" a@example.com "})).as_deref(),
        Some("a@example.com")
    );
    assert_eq!(email(json!({"emailAddress":"  "})), None);
    assert_eq!(email(Value::Null), None);
}

/// A Claude credential generation is its access and refresh tokens and nothing else. The digest
/// is a literal because a vault's signed-out generations and a renewal journal written by an
/// earlier version must still match the login they name.
#[test]
fn a_claude_logins_fingerprint_is_its_token_generation_alone() {
    let claude = login(
        json!({"claudeAiOauth":{"accessToken":"access-a","refreshToken":"refresh-a","expiresAt":1}}),
        json!({"accountUuid":"a","emailAddress":"a@example.com"}),
    );
    let fingerprint = |login: &Login| ClaudeLogin::of(login).fingerprint();
    assert_eq!(
        fingerprint(&claude),
        "fdeed22ee434f77cea8634af474c4d1aaa71da120a95eb7f4f5015fbf4a765bf"
    );
    let mut presented = claude.clone();
    presented.account["emailAddress"] = json!("renamed@example.com");
    presented.auth["claudeAiOauth"]["expiresAt"] = json!(2);
    presented.auth["claudeAiOauth"]["subscriptionType"] = json!("max");
    assert_eq!(fingerprint(&presented), fingerprint(&claude));
    let mut rotated = claude.clone();
    rotated.auth["claudeAiOauth"]["refreshToken"] = json!("refresh-b");
    assert_ne!(fingerprint(&rotated), fingerprint(&claude));
}
