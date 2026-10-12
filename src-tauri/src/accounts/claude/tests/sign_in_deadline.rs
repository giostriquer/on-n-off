use super::*;

const ENDS_AT: i64 = 1_791_891_940_000;

fn account() -> Value {
    json!({"accountUuid":"user-a","organizationUuid":"org-a","emailAddress":"a@example.com"})
}

fn stored(root: &Path, credentials: &Value, config: &Value) -> ClaudeNative {
    let native = claude(root);
    fs::write(
        native.config_home.join(".credentials.json"),
        credentials.to_string(),
    )
    .unwrap();
    fs::write(&native.config_file, config.to_string()).unwrap();
    native
}

fn deadline(at: i64) -> Option<SignInDeadline> {
    Some(SignInDeadline {
        identity: Identity {
            provider: AgentId::Claude,
            user_id: "user-a".into(),
            workspace_id: "org-a".into(),
        },
        ends_at_ms: at,
    })
}

#[test]
fn the_sign_in_deadline_is_the_one_claude_code_stored_with_its_login() {
    let root = tempfile::tempdir().unwrap();
    let native = stored(
        root.path(),
        &json!({"claudeAiOauth":{
            "accessToken":"access","refreshToken":"refresh",
            "expiresAt":1_791_850_000_000_i64,"refreshTokenExpiresAt":ENDS_AT
        }}),
        &json!({"oauthAccount": account()}),
    );

    assert_eq!(NativeAccount::sign_in_deadline(&native), deadline(ENDS_AT));
}

#[test]
fn a_login_claude_code_gave_up_on_keeps_the_deadline_it_ended_at() {
    let root = tempfile::tempdir().unwrap();
    let native = stored(
        root.path(),
        &json!({"claudeAiOauth":{
            "accessToken":"","refreshToken":"","expiresAt":0,
            "refreshTokenExpiresAt":ENDS_AT,"scopes":["user:inference"],"subscriptionType":"max"
        }}),
        &json!({"oauthAccount": account()}),
    );

    assert!(native.read().unwrap().is_none(), "no usable login is left");
    assert_eq!(NativeAccount::sign_in_deadline(&native), deadline(ENDS_AT));
}

#[test]
fn there_is_no_deadline_without_a_stored_date_and_the_account_it_is_for() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        NativeAccount::sign_in_deadline(&claude(root.path())),
        None,
        "no store"
    );

    let dated = json!({"claudeAiOauth":{"accessToken":"a","refreshToken":"r","refreshTokenExpiresAt":ENDS_AT}});
    for (credentials, config, why) in [
        (
            serde_json::from_str(SIGNED_OUT).unwrap(),
            json!({"oauthAccount": account()}),
            "signed out by on-n-off",
        ),
        (
            json!({"claudeAiOauth":{"accessToken":"a","refreshToken":"r"}}),
            json!({"oauthAccount": account()}),
            "no date stored",
        ),
        (
            json!({"claudeAiOauth":{"accessToken":"a","refreshToken":"r","refreshTokenExpiresAt":"soon"}}),
            json!({"oauthAccount": account()}),
            "a date that is not a number",
        ),
        (dated.clone(), json!({}), "no account record"),
        (
            dated,
            json!({"oauthAccount":{"accountUuid":"user-a"}}),
            "an account without its organization",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let native = stored(root.path(), &credentials, &config);
        assert_eq!(NativeAccount::sign_in_deadline(&native), None, "{why}");
    }
}
