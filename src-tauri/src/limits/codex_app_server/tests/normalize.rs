//! The card an app-server read becomes: which account it is, confirmed against the native login
//! before and after the read, and the access projection that confirmation hands the backend reads.

use super::*;

#[test]
fn normalizes_the_chatgpt_account_and_rate_limits_without_reading_a_token() {
    let codex_home = crate::paths::scratch_dir("codex-app-server-normalize");
    std::fs::write(
        codex_home.join("auth.json"),
        r#"{"tokens":{"account_id":"acct-1"}}"#,
    )
    .unwrap();
    let before = crate::accounts::codex_store::metadata(&codex_home).unwrap();
    let parsed = normalize_app_server(
        AppServerResult {
            codex_home,
            account: typed(json!({
                "account": {
                    "type": "chatgpt",
                    "email": "Me@Example.com",
                    "planType": "pro"
                },
                "requiresOpenaiAuth": true
            })),
            rate_limits: typed(json!({
                "rateLimits": {
                    "limitId": "codex",
                    "primary": {
                        "usedPercent": 42,
                        "windowDurationMins": 10080,
                        "resetsAt": 1787838960
                    },
                    "secondary": null,
                    "credits": {"hasCredits": false, "unlimited": false, "balance": "0"},
                    "planType": "pro"
                }
            })),
        },
        before,
    )
    .unwrap()
    .0;

    assert_eq!(
        parsed.account,
        Some(crate::dto::LimitsAccountDto {
            legacy_id: None,
            id: "acct-1".to_string(),
            label: Some("Me@Example.com".to_string()),
        })
    );
    assert_eq!(parsed.reading.plan.as_deref(), Some("pro"));
    assert_eq!(parsed.reading.windows.len(), 1);
    assert_eq!(parsed.reading.windows[0].used_percent, 42.0);
}

#[test]
fn falls_back_to_a_normalized_email_identity_when_codex_has_no_account_id() {
    let codex_home = crate::paths::scratch_dir("codex-app-server-email");
    let before = crate::accounts::codex_store::metadata(&codex_home).unwrap();
    let parsed = normalize_app_server(
        AppServerResult {
            codex_home,
            account: typed(json!({
                "account": {
                    "type": "chatgpt",
                    "email": " Me@Example.com ",
                    "planType": "plus"
                },
                "requiresOpenaiAuth": true
            })),
            rate_limits: typed(json!({"rateLimits": {"planType": "plus"}})),
        },
        before,
    )
    .unwrap()
    .0;

    assert_eq!(
        parsed.account,
        Some(crate::dto::LimitsAccountDto {
            legacy_id: None,
            id: "email:me@example.com".to_string(),
            label: Some("Me@Example.com".to_string()),
        })
    );
}

#[test]
fn api_key_accounts_are_explicitly_unsupported() {
    let error = normalize_app_server(
        AppServerResult {
            codex_home: crate::paths::scratch_dir("codex-app-server-api-key"),
            account: typed(json!({
                "account": {"type": "apiKey"},
                "requiresOpenaiAuth": false
            })),
            rate_limits: typed(json!({"rateLimits": {}})),
        },
        None,
    )
    .err()
    .unwrap();

    assert!(matches!(
        error,
        AppServerFailure::Unsupported(message) if message.contains("API key")
    ));
}

#[test]
fn scoped_codex_observation_carries_its_previous_workspace_key() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let codex_home = crate::paths::scratch_dir("codex-app-server-scoped-legacy");
    let payload = json!({"https://api.openai.com/auth": {"chatgpt_user_id":"user-1", "chatgpt_account_id":"workspace-1"}});
    let token = format!(
        "header.{}.signature",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap())
    );
    std::fs::write(
        codex_home.join("auth.json"),
        json!({"tokens":{"account_id":"workspace-1", "id_token": token}}).to_string(),
    )
    .unwrap();
    let before = crate::accounts::codex_store::metadata(&codex_home).unwrap();
    let parsed = normalize_app_server(AppServerResult {
        codex_home,
        account: typed(json!({"account":{"type":"chatgpt", "email":"me@example.com", "planType":"pro"}, "requiresOpenaiAuth":true})),
        rate_limits: typed(json!({"rateLimits":{"planType":"pro"}})),
    }, before).unwrap()
    .0;
    let account = parsed.account.unwrap();
    assert!(account.id.starts_with("profile:"));
    assert_eq!(account.legacy_id.as_deref(), Some("workspace-1"));
    assert_eq!(account.label.as_deref(), Some("me@example.com"));
}

#[test]
fn rejects_native_identity_changes_during_an_app_server_read() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    for plan in ["pro", "business"] {
        for (user, workspace) in [("user-2", "workspace-1"), ("user-1", "workspace-2")] {
            let codex_home = crate::paths::scratch_dir("codex-app-server-account-race");
            let write_identity = |user: &str, workspace: &str| {
                let payload = json!({"https://api.openai.com/auth": {"chatgpt_user_id":user, "chatgpt_account_id":workspace}});
                let token = format!(
                    "header.{}.signature",
                    URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap())
                );
                std::fs::write(
                    codex_home.join("auth.json"),
                    json!({"tokens":{"account_id":workspace,"id_token":token,"access_token":"fixture-access"}})
                        .to_string(),
                )
                .unwrap();
            };
            write_identity("user-1", "workspace-1");
            let before = crate::accounts::codex_store::metadata(&codex_home).unwrap();
            let mut transport = FakeTransport {
                received: VecDeque::from([
                    json!({"id":1,"result":{"codexHome":codex_home}}),
                    json!({"id":2,"result":{"account":{"type":"chatgpt","email":"same@example.com","planType":plan}}}),
                    json!({"id":3,"result":{"rateLimits":{"primary":{"usedPercent":42,"windowDurationMins":10080}}}}),
                ]),
                sent: vec![],
            };
            let session = query_app_server(&codex_home, false, &mut transport).unwrap();
            write_identity(user, workspace);
            assert!(
                matches!(normalize_app_server(session, before), Err(AppServerFailure::Failed(message)) if message.contains("changed")),
                "{plan}: {user} / {workspace}"
            );
        }
    }
}

/// A session for `plan` whose native login holds an access token.
fn signed_in_on(plan: &str, name: &str) -> (AppServerResult, Option<(String, Value)>) {
    let codex_home = business_home(name).join(".codex");
    let before = crate::accounts::codex_store::metadata(&codex_home).unwrap();
    let session = AppServerResult {
        codex_home,
        account: typed(json!({
            "account": {"type": "chatgpt", "email": "you@example.com", "planType": plan},
            "requiresOpenaiAuth": true
        })),
        rate_limits: typed(json!({"rateLimits": {"planType": plan}})),
    };
    (session, before)
}

/// The identity check after the handshake reads the native store once, and for a workspace plan
/// that one read also yields the login's access projection, so spending costs no read of its own.
#[test]
fn a_workspace_plan_takes_its_access_from_the_identity_check() {
    let (session, before) = signed_in_on("business", "codex-app-server-access");

    let (parsed, access) = normalize_app_server(session, before).unwrap();

    let access = access.unwrap();
    assert_eq!(
        Some(access.observation_key.as_str()),
        parsed.account.as_ref().map(|a| a.id.as_str())
    );
    assert_eq!(access.workspace_id, "acct-1");
    assert_eq!(access.token.authorization(), "Bearer fixture-access");
}

#[test]
fn a_personal_plan_takes_the_access_projection_too_for_the_term_read() {
    let (session, before) = signed_in_on("pro", "codex-app-server-no-access");

    let (parsed, access) = normalize_app_server(session, before).unwrap();

    assert_eq!(
        access.map(|access| access.observation_key),
        Some("acct-1".to_string())
    );
    assert_eq!(parsed.account.unwrap().id, "acct-1");
}

/// The account's plan outranks the rate-limit bucket's.
#[test]
fn the_accounts_plan_decides_the_card() {
    let codex_home = business_home("codex-app-server-plan-precedence").join(".codex");
    let before = crate::accounts::codex_store::metadata(&codex_home).unwrap();
    let session = AppServerResult {
        codex_home,
        account: typed(json!({
            "account": {"type": "chatgpt", "email": "you@example.com", "planType": "business"},
            "requiresOpenaiAuth": true
        })),
        rate_limits: typed(json!({"rateLimits": {"planType": "pro"}})),
    };

    let (parsed, access) = normalize_app_server(session, before).unwrap();

    assert_eq!(parsed.reading.plan.as_deref(), Some("business"));
    assert_eq!(
        access.map(|access| access.token.authorization()),
        Some("Bearer fixture-access".to_string())
    );
}
