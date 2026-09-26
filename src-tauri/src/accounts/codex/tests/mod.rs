use super::*;
use serde_json::json;

/// What account changes say about a native home the environment chose.
const CUSTOM_HOME: &str = "Account activation currently supports the default CLI home. Remove the custom home override or use the official CLI for this context.";

/// An environment holding exactly `vars`, for `resolve_from`.
fn environment<'a>(
    vars: &'a [(&'a str, PathBuf)],
) -> impl Fn(&str) -> Option<std::ffi::OsString> + 'a {
    move |name| {
        vars.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.clone().into_os_string())
    }
}

/// The environment a command reads, as the child will see it: `Some(None)` is a variable removed.
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

/// An unsigned ID token whose `https://api.openai.com/auth` claims are `claims`, shaped the way
/// `CodexLogin` decodes it: fixtures across the crate build their logins from it.
pub(crate) fn id_token(claims: &Value) -> String {
    jwt(&json!({"https://api.openai.com/auth": claims}))
}

/// An unsigned JWT whose payload is `payload`.
fn jwt(payload: &Value) -> String {
    format!(
        "header.{}.signature",
        URL_SAFE_NO_PAD.encode(payload.to_string())
    )
}

fn auth(user: &str, workspace: &str) -> Value {
    let claims = json!({"sub":user,"https://api.openai.com/auth":{"chatgpt_user_id":user,"chatgpt_account_id":workspace}});
    json!({"tokens":{"id_token":format!("e30.{}.sig",URL_SAFE_NO_PAD.encode(claims.to_string())),"access_token":"access","refresh_token":"renewable","account_id":workspace}})
}

/// A Codex login of `auth`, with no account record beside it, as Codex keeps none.
fn login_of(auth: &Value) -> Login {
    Login {
        auth: auth.clone(),
        account: Value::Null,
    }
}

fn identity_of(auth: &Value) -> Result<Identity, String> {
    CodexLogin::of(&login_of(auth)).identity()
}

#[test]
fn scopes_profiles_to_both_user_and_workspace() {
    let a = identity_of(&auth("user-a", "team")).unwrap();
    let b = identity_of(&auth("user-b", "team")).unwrap();
    let personal = identity_of(&auth("user-a", "personal")).unwrap();
    assert_ne!(a, b);
    assert_ne!(a, personal);
    assert_eq!(a.provider, AgentId::Codex);
    assert_eq!(a.user_id, "user-a");
    assert_eq!(a.workspace_id, "team");
}

#[test]
fn refuses_codex_workspace_claim_disagreement_and_access_only_login() {
    let mut value = auth("user-a", "team");
    value["tokens"]["account_id"] = json!("other");
    assert!(identity_of(&value).is_err());
    let mut value = auth("user-a", "team");
    value["tokens"]
        .as_object_mut()
        .unwrap()
        .remove("refresh_token");
    assert!(identity_of(&value).is_err());
}

/// A Codex login that carries an API key is not a subscription, so it is never a profile; a key
/// left null is no key.
#[test]
fn a_codex_api_key_login_is_not_a_subscription_profile() {
    let mut value = auth("user-a", "team");
    value["OPENAI_API_KEY"] = json!("fixture-api-key");
    assert_eq!(
        identity_of(&value).err().as_deref(),
        Some("API key logins cannot be saved as subscription profiles.")
    );
    value["OPENAI_API_KEY"] = Value::Null;
    assert!(identity_of(&value).is_ok());
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
    let renews_soon = |auth: &Value| CodexLogin::of(&login_of(auth)).renews_soon(1_000_000);
    assert!(!renews_soon(&expiring_at(1_000_600)));
    assert!(renews_soon(&expiring_at(1_000_599)));
    assert!(renews_soon(&auth("user-a", "team")));
}

/// A Codex login's email is the ID token's own `email` claim, trimmed: not one nested under the
/// auth claims, and none without an ID token.
#[test]
fn a_codex_logins_email_is_the_id_tokens_own_claim() {
    let email = |auth: Value| CodexLogin::of(&login_of(&auth)).email();
    let with = |payload: Value| json!({"tokens":{"access_token":"access","refresh_token":"refresh","id_token":jwt(&payload)}});
    assert_eq!(
        email(with(json!({"email":" c@example.com "}))).as_deref(),
        Some("c@example.com")
    );
    assert_eq!(
        email(with(
            json!({"https://api.openai.com/auth":{"email":"nested@example.com"}})
        )),
        None,
        "only the top-level claim is the email"
    );
    assert_eq!(
        CodexLogin::of(&Login {
            auth: json!({"tokens":{"access_token":"access"}}),
            account: json!({"emailAddress":"a@example.com"}),
        })
        .email(),
        None,
        "a Codex login without an ID token has no email, whatever lies beside it"
    );
}

/// A Codex credential generation is its access and refresh tokens and nothing else. The digest is
/// a literal because a vault's signed-out generations and a renewal journal written by an earlier
/// version must still match the login they name.
#[test]
fn a_codex_logins_fingerprint_is_its_token_generation_alone() {
    let codex = json!({"tokens":{"access_token":"access-c","refresh_token":"refresh-c","id_token":"id-one","account_id":"team"},"last_refresh":"2026-09-01T00:00:00Z"});
    let fingerprint = |auth: &Value| CodexLogin::of(&login_of(auth)).fingerprint();
    assert_eq!(
        fingerprint(&codex),
        "4ca39507b5d18d079c34d4882ed151f283570622c6f465583e7972fa0e8bce4a"
    );
    let mut reissued = codex.clone();
    reissued["tokens"]["id_token"] = json!("id-two");
    reissued["last_refresh"] = json!("2026-09-02T00:00:00Z");
    assert_eq!(fingerprint(&reissued), fingerprint(&codex));
    let mut renewed = codex.clone();
    renewed["tokens"]["access_token"] = json!("access-d");
    assert_ne!(fingerprint(&renewed), fingerprint(&codex));
}

#[cfg(target_os = "macos")]
mod keychain;
mod native_store;
