use super::*;
use std::fs;

#[test]
fn scoped_codex_metadata_keeps_same_workspace_users_separate() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let root = tempfile::tempdir().unwrap();
    let save = |user: &str| {
        let claims = json!({"https://api.openai.com/auth":{"chatgpt_account_id":"team","chatgpt_user_id":user}});
        fs::write(root.path().join("auth.json"),json!({"tokens":{"account_id":"team","id_token":format!("e30.{}.s",URL_SAFE_NO_PAD.encode(claims.to_string()))}}).to_string()).unwrap();
    };
    save("a");
    let a = metadata(root.path()).unwrap().unwrap().0;
    save("b");
    let b = metadata(root.path()).unwrap().unwrap().0;
    assert_ne!(a, b);
    assert!(a.starts_with("profile:"));
    fs::write(
        root.path().join("config.toml"),
        "cli_auth_credentials_store = 'ephemeral'",
    )
    .unwrap();
    assert!(metadata(root.path()).is_err());
}

/// A Codex login on disk, as the CLI writes it: its tokens under `tokens`, the workspace's claims in
/// the id token.
fn codex_login(root: &Path, access_token: Option<&str>) {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let claims = json!({"https://api.openai.com/auth":{"chatgpt_account_id":"team","chatgpt_user_id":"user"}});
    let mut tokens = json!({
        "account_id": "team",
        "refresh_token": "fixture-refresh",
        "id_token": format!("e30.{}.s", URL_SAFE_NO_PAD.encode(claims.to_string())),
    });
    if let Some(token) = access_token {
        tokens["access_token"] = json!(token);
    }
    fs::write(
        root.join("auth.json"),
        json!({"tokens": tokens}).to_string(),
    )
    .unwrap();
}

/// Only the access token leaves accounts, beside the key the card is known by and its workspace.
#[test]
fn the_access_projection_carries_only_the_access_token_with_the_cards_identity() {
    let root = tempfile::tempdir().unwrap();
    codex_login(root.path(), Some("fixture-access"));

    let (projected, access) = metadata_and_access(root.path()).unwrap().unwrap();
    let access = access.unwrap();

    assert_eq!(Some(&projected), metadata(root.path()).unwrap().as_ref());
    assert_eq!(access.observation_key, projected.0);
    assert_eq!(access.workspace_id, "team");
    assert_eq!(access.token.authorization(), "Bearer fixture-access");
}

#[test]
fn a_codex_login_without_an_access_token_gives_its_identity_and_no_access() {
    let root = tempfile::tempdir().unwrap();
    codex_login(root.path(), None);

    let (projected, access) = metadata_and_access(root.path()).unwrap().unwrap();
    assert_eq!(Some(projected), metadata(root.path()).unwrap());
    assert!(access.is_none());
    assert!(metadata_and_access(tempfile::tempdir().unwrap().path())
        .unwrap()
        .is_none());
}

/// Reading the token must not loosen the identity rules `metadata` enforces.
#[test]
fn the_access_projection_refuses_a_login_whose_claims_name_another_workspace() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let root = tempfile::tempdir().unwrap();
    let claims = json!({"https://api.openai.com/auth":{"chatgpt_account_id":"other","chatgpt_user_id":"user"}});
    fs::write(
        root.path().join("auth.json"),
        json!({"tokens":{"account_id":"team","access_token":"fixture-access",
            "id_token":format!("e30.{}.s",URL_SAFE_NO_PAD.encode(claims.to_string()))}})
        .to_string(),
    )
    .unwrap();

    assert!(metadata_and_access(root.path()).is_err());
}

/// Codex's keyring target on macOS sends `security` exactly the commands the shared writer
/// builds, and nothing else: the login as one `add-generic-password -U` with the secret
/// hex-encoded, a removal as one `delete-generic-password` naming account and service. A write
/// back through the `keyring` crate — this process's own identity, which is what prompted on
/// every switch — would send nothing here and fail this test.
#[cfg(target_os = "macos")]
#[test]
fn the_keyring_target_writes_and_deletes_through_security() {
    use crate::accounts::keychain::{with_test_runner, Runner};
    use crate::process::CommandOutcome;

    let ok: Runner = |_| CommandOutcome::Exited {
        success: true,
        stdout: String::new(),
        stderr: String::new(),
    };
    // Synthetic names on purpose: anyone re-checking this guard by putting the `keyring` crate
    // back would otherwise file a second item under Claude Code's own service, which the
    // service-only read could then return instead of the real login.
    let target = Target::Keyring {
        service: "on-n-off seam rehearsal".into(),
        account: "on-n-off-test".into(),
    };
    let login = json!({"claudeAiOauth": {"accessToken": "one", "note": "a \"quoted\" word"}});
    let hex: String = serde_json::to_vec(&login)
        .unwrap()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();

    let (result, sent) = with_test_runner(ok, || target.write(Some(&login)));
    assert_eq!(result, Ok(()));
    assert_eq!(
        sent,
        vec![format!(
            "add-generic-password -U -a \"on-n-off-test\" -s \"on-n-off seam rehearsal\" -X \"{hex}\"\n"
        )]
    );

    let (result, sent) = with_test_runner(ok, || target.write(None));
    assert_eq!(result, Ok(()));
    assert_eq!(
        sent,
        vec![
            "delete-generic-password -a \"on-n-off-test\" -s \"on-n-off seam rehearsal\"\n"
                .to_string()
        ]
    );

    let refused: Runner = |_| CommandOutcome::Exited {
        success: false,
        stdout: String::new(),
        stderr: "User interaction is not allowed.".to_string(),
    };
    let (result, _) = with_test_runner(refused, || target.write(Some(&login)));
    assert_eq!(
        result,
        Err("Keychain write failed (User interaction is not allowed).".to_string()),
        "the refusal reads as a sentence, for the transaction to prefix its own"
    );
}

/// The same path against the real tool, on a throwaway entry: publish, read back, remove, remove
/// again. What it proves beyond the test above is the wiring to `security` itself.
///
/// `cargo test --manifest-path src-tauri/Cargo.toml rehearse_the_keyring_target -- --ignored`
#[cfg(target_os = "macos")]
#[test]
#[ignore = "writes a throwaway Keychain entry; not part of CI"]
fn rehearse_the_keyring_target_through_security() {
    crate::accounts::with_real_keychain(rehearse_the_keyring_target);
}

#[cfg(target_os = "macos")]
fn rehearse_the_keyring_target() {
    let entry = crate::accounts::keychain::ThrowawayEntry {
        service: "on-n-off native keychain rehearsal",
        account: "on-n-off-test",
    };
    let target = Target::Keyring {
        service: entry.service.into(),
        account: entry.account.into(),
    };
    let login = json!({"claudeAiOauth": {"accessToken": "one", "note": "a \"quoted\" word"}});
    target.write(Some(&login)).unwrap();
    assert_eq!(target.read().unwrap(), Some(login));
    target.write(None).unwrap();
    assert_eq!(target.read().unwrap(), None);
    target.write(None).unwrap();
    assert_eq!(
        target.read().unwrap(),
        None,
        "removing an entry already gone is not an error"
    );
    drop(entry);
}
