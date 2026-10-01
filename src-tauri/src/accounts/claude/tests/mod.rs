use super::*;
use serde_json::json;

const CUSTOM_HOME: &str = "Account activation currently supports the default CLI home. Remove the custom home override or use the official CLI for this context.";

fn claude(root: &Path) -> ClaudeNative {
    let home = root.join(".claude");
    fs::create_dir_all(&home).unwrap();
    ClaudeNative {
        config_home: home,
        config_file: root.join(".claude.json"),
        custom: false,
        use_keychain: false,
        secure_storage: None,
        in_home: false,
        private: false,
    }
}
fn environment<'a>(
    vars: &'a [(&'a str, PathBuf)],
) -> impl Fn(&str) -> Option<std::ffi::OsString> + 'a {
    move |name| {
        vars.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.clone().into_os_string())
    }
}

fn command_env(command: &Command) -> std::collections::HashMap<String, Option<std::ffi::OsString>> {
    command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(std::ffi::OsStr::to_os_string),
            )
        })
        .collect()
}

const SIGNED_OUT: &str = r#"{"claudeAiOauth":{"accessToken":"","refreshToken":"","expiresAt":0}}"#;

fn incoming() -> Login {
    Login {
        auth: json!({"claudeAiOauth":{"accessToken":"incoming"}}),
        account: json!({"accountUuid":"b","organizationUuid":"org-b"}),
    }
}

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

#[test]
fn a_claude_login_is_due_to_renew_once_its_expiry_is_reached() {
    for (expires_at, due) in [
        (Some(1_000_000), true),
        (Some(1_000_001), false),
        (None, false),
    ] {
        let mut auth = json!({"claudeAiOauth":{"accessToken":"access","refreshToken":"refresh"}});
        if let Some(at) = expires_at {
            auth["claudeAiOauth"]["expiresAt"] = json!(at);
        }
        let claude = login(
            auth,
            json!({"accountUuid":"user","organizationUuid":"team"}),
        );
        assert_eq!(
            ClaudeLogin::of(&claude).renewal_due(1_000_000),
            due,
            "expiring at {expires_at:?}"
        );
    }
}

mod first_usage;
mod home;
#[cfg(target_os = "macos")]
mod keychain;
mod native_store;
mod secure_storage;
