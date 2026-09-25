use super::*;
use crate::http::{refused_url, serve_once_capturing, serve_once_observing};
use crate::paths::scratch_dir;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const NOW_MS: i64 = 1_787_000_000_000;

fn stored() -> Value {
    json!({
        "claudeAiOauth": {
            "accessToken": "old",
            "refreshToken": "rt",
            "expiresAt": NOW_MS - 1,
            "refreshTokenExpiresAt": NOW_MS + 600_000,
            "scopes": ["user:profile", "user:inference"],
            "subscriptionType": "max",
            "rateLimitTier": "default_claude_max_5x",
        }
    })
}

fn reply() -> Value {
    json!({
        "access_token": "new",
        "refresh_token": "rt2",
        "expires_in": 28_800,
        "refresh_token_expires_in": 604_800,
        "scope": "user:profile user:inference",
    })
}

const REPLY_JSON: &str = r#"{"access_token":"new","refresh_token":"rt2","expires_in":28800,"refresh_token_expires_in":604800,"scope":"user:profile user:inference"}"#;

/// A home whose only store is a credentials file holding `document`.
fn home_with(prefix: &str, document: &Value) -> PathBuf {
    let home = scratch_dir(prefix);
    let path = home.join(".claude").join(".credentials.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, document.to_string()).unwrap();
    home
}

fn login(home: &Path, token_url: &str) -> CredentialLookup<ClaudeCredential> {
    current_login(
        &StorageDir::default_in(home),
        &|_: &StorageDir| Ok(None),
        NOW_MS,
        token_url,
    )
}

fn stored_token(home: &Path) -> String {
    let raw = fs::read_to_string(home.join(".claude").join(".credentials.json")).unwrap();
    serde_json::from_str::<Value>(&raw).unwrap()["claudeAiOauth"]["accessToken"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn a_renewal_replaces_the_tokens_and_leaves_everything_else_alone() {
    let mut document = stored();
    apply(&mut document, &reply(), NOW_MS).unwrap();

    let oauth = &document["claudeAiOauth"];
    assert_eq!(oauth["accessToken"], "new");
    assert_eq!(oauth["refreshToken"], "rt2");
    assert_eq!(oauth["expiresAt"].as_i64(), Some(NOW_MS + 28_800_000));
    assert_eq!(
        oauth["refreshTokenExpiresAt"].as_i64(),
        Some(NOW_MS + 604_800_000)
    );
    assert_eq!(
        oauth["subscriptionType"], "max",
        "the reply says nothing about the plan, so the stored one stands"
    );
    assert_eq!(
        oauth["rateLimitTier"], "default_claude_max_5x",
        "a field this code does not know about still has to survive the write"
    );

    let credential = credentials::parse_claude_credential(&document).unwrap();
    assert_eq!(credential.token, "new");
    assert_eq!(credential.expires_at_ms, Some(NOW_MS + 28_800_000));
    assert!(credential.has_refresh_token);
    assert_eq!(credential.subscription_type.as_deref(), Some("max"));
}

/// The issuer rotates the refresh token on most renewals but not on all of them. Dropping the
/// stored one when the reply is silent would sign the user out at the next expiry.
#[test]
fn a_reply_without_a_rotated_refresh_token_keeps_the_stored_one() {
    let mut document = stored();
    let mut reply = reply();
    reply.as_object_mut().unwrap().remove("refresh_token");
    apply(&mut document, &reply, NOW_MS).unwrap();
    assert_eq!(document["claudeAiOauth"]["refreshToken"], "rt");
}

#[test]
fn a_reply_missing_the_token_or_its_lifetime_is_refused_rather_than_stored() {
    for missing in ["access_token", "expires_in"] {
        let mut document = stored();
        let mut reply = reply();
        reply.as_object_mut().unwrap().remove(missing);
        assert!(
            apply(&mut document, &reply, NOW_MS).is_err(),
            "{missing} is not optional"
        );
        assert_eq!(
            document["claudeAiOauth"]["accessToken"], "old",
            "a refused reply leaves the stored login untouched"
        );
    }
}

#[test]
fn the_grant_names_claude_codes_client_and_the_scopes_the_login_was_issued() {
    let oauth = &stored()["claudeAiOauth"];
    let body = request_body("rt", &scopes(oauth), client_id(oauth));
    assert_eq!(body["grant_type"], "refresh_token");
    assert_eq!(body["refresh_token"], "rt");
    assert_eq!(body["client_id"], CLIENT_ID);
    assert_eq!(body["scope"], "user:profile user:inference");
}

#[test]
fn a_login_that_names_no_scopes_asks_for_the_ones_claude_code_asks_for() {
    assert_eq!(scopes(&json!({})), DEFAULT_SCOPES.to_vec());
    assert_eq!(scopes(&json!({ "scopes": [] })), DEFAULT_SCOPES.to_vec());
}

/// The whole path, through the store the read chose: expired login in, renewed login out, and the
/// file left holding what the caller was handed.
#[test]
fn an_expired_login_is_renewed_and_the_renewal_is_what_gets_stored() {
    let home = home_with("renew-round-trip", &stored());
    let (token_url, request) = serve_once_capturing("200 OK", &[], REPLY_JSON);

    let CredentialLookup::Found(credential) = login(&home, &token_url) else {
        panic!("expected a renewed login");
    };
    request.join().unwrap();
    assert_eq!(credential.token, "new");

    let path = home.join(".claude").join(".credentials.json");
    let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written["claudeAiOauth"]["accessToken"], "new");
    assert_eq!(
        credentials::parse_claude_credential(&written).unwrap(),
        credential,
        "the credential handed back is the one on disk, not a parallel reconstruction"
    );
    assert!(
        !path.with_extension("json.on-n-off").exists(),
        "the temporary is renamed away, never left holding a refresh token"
    );
    assert!(
        !home.join(".claude").join(".oauth_refresh.lock").exists()
            && !home.join(".claude.lock").exists(),
        "the locks are released once the renewal is done"
    );
}

/// Losing the race is the lock working: the winner has already stored a good token, and redeeming
/// a second one would spend a refresh token for nothing.
#[test]
fn a_login_another_process_renewed_while_we_waited_is_used_as_it_stands() {
    let mut document = stored();
    document["claudeAiOauth"]["accessToken"] = json!("theirs");
    document["claudeAiOauth"]["expiresAt"] = json!(NOW_MS + 1);
    let home = home_with("renew-lost-race", &document);

    // A refused endpoint: reaching the network at all would be the bug.
    let credential = renew(
        &StorageDir::default_in(&home),
        &|_: &StorageDir| Ok(None),
        NOW_MS,
        &refused_url(),
        &RefusedLogin::new(),
    )
    .unwrap();
    assert_eq!(credential.token, "theirs");
}

/// While Claude Code holds the lock it is renewing the same login, so the next poll reads what it
/// wrote. Nothing is redeemed here and the message the user already had stands.
#[test]
fn a_held_lock_leaves_the_renewal_to_whoever_holds_it() {
    let home = home_with("renew-busy", &stored());
    let _held = ClaudeLocks::acquire(&StorageDir::default_in(&home), LockScope::Refresh).unwrap();

    assert_eq!(
        renew(
            &StorageDir::default_in(&home),
            &|_: &StorageDir| Ok(None),
            NOW_MS,
            &refused_url(),
            &RefusedLogin::new()
        )
        .unwrap_err(),
        RenewError::Busy,
        "a refused endpoint would have reported differently, so nothing was sent"
    );
    assert_eq!(
        login(&home, &refused_url()),
        CredentialLookup::Expired { renewable: true }
    );
}

/// An issuer we cannot reach spends nothing and says nothing about the login, so the advice the
/// user already had is still the right advice.
#[test]
fn an_unreachable_issuer_keeps_the_advice_the_user_already_had() {
    let home = home_with("renew-offline", &stored());

    assert!(matches!(
        renew(
            &StorageDir::default_in(&home),
            &|_: &StorageDir| Ok(None),
            NOW_MS,
            &refused_url(),
            &RefusedLogin::new()
        ),
        Err(RenewError::Unavailable(_))
    ));
    assert_eq!(
        login(&home, &refused_url()),
        CredentialLookup::Expired { renewable: true }
    );
    assert_eq!(stored_token(&home), "old", "the store is untouched");
}

/// `invalid_grant`: the refresh token is spent or revoked, and no amount of running `claude` will
/// renew it. The user has to sign in again, and the message has to say so.
#[test]
fn a_refused_refresh_token_asks_for_a_new_sign_in_rather_than_a_renewal() {
    let home = home_with("renew-rejected", &stored());
    let (token_url, request) = serve_once_capturing("400 Bad Request", &[], r#"{"error":"x"}"#);

    assert_eq!(
        login(&home, &token_url),
        CredentialLookup::Expired { renewable: false }
    );
    request.join().unwrap();
}

/// A refusal is permanent for that stored login, so posting it again would achieve nothing except
/// taking Claude Code's refresh locks every five minutes for as long as the user leaves
/// it alone. A login Claude Code has since rewritten is a different login, and worth trying.
#[test]
fn the_same_refused_login_is_not_sent_a_second_time() {
    let home = home_with("renew-refused-memo", &stored());
    let refused = RefusedLogin::new();
    let (token_url, request) = serve_once_capturing("400 Bad Request", &[], r#"{"error":"x"}"#);

    let attempt = |url: &str| {
        renew(
            &StorageDir::default_in(&home),
            &|_: &StorageDir| Ok(None),
            NOW_MS,
            url,
            &refused,
        )
    };
    assert_eq!(attempt(&token_url).unwrap_err(), RenewError::Rejected);
    request.join().unwrap();

    // The one-shot server is gone: connecting again would report `Unavailable`, not `Rejected`.
    assert_eq!(attempt(&token_url).unwrap_err(), RenewError::Rejected);

    let mut rewritten = stored();
    rewritten["claudeAiOauth"]["expiresAt"] = json!(NOW_MS - 2);
    fs::write(
        home.join(".claude").join(".credentials.json"),
        rewritten.to_string(),
    )
    .unwrap();
    let (token_url, request) = serve_once_capturing("200 OK", &[], REPLY_JSON);
    assert_eq!(attempt(&token_url).unwrap().token, "new");
    request.join().unwrap();
}

/// The dangerous case: redeemed, then not stored. The refresh token that bought the new login is
/// spent, so "run `claude` to renew it" is advice that cannot work, and the user is owed both the
/// fact that on-n-off caused it and the reason.
#[test]
fn a_reply_that_is_not_a_token_strands_the_login_and_says_why() {
    let home = home_with("renew-stranded", &stored());
    let (token_url, request) = serve_once_capturing("200 OK", &[], r#"{"ok":true}"#);

    let CredentialLookup::Stranded(why) = login(&home, &token_url) else {
        panic!("a 2xx that carries no token is not something to retry");
    };
    request.join().unwrap();
    assert!(why.contains("access_token"), "{why}");
    assert_eq!(stored_token(&home), "old");
}

/// A 2xx we cannot parse is the same hazard: `post_grant` only parses on success, so the issuer
/// almost certainly did rotate the token behind the reply we lost.
#[test]
fn an_unreadable_success_is_treated_as_a_spent_token_not_a_retry() {
    let home = home_with("renew-unparseable", &stored());
    let (token_url, request) = serve_once_capturing("200 OK", &[], "<html>gateway</html>");

    assert!(matches!(
        login(&home, &token_url),
        CredentialLookup::Stranded(_)
    ));
    request.join().unwrap();
}

/// Everything that can fail about the write for reasons unrelated to the reply has to fail before
/// the grant is sent, because after it there is no un-renewed state to fall back to. Here the
/// login is in the Keychain and the entry's account cannot be found to write it back under.
#[cfg(target_os = "macos")]
#[test]
fn a_write_that_cannot_be_prepared_fails_before_anything_is_spent() {
    use crate::accounts::keychain::with_test_runner;
    use crate::process::CommandOutcome;
    let home = scratch_dir("renew-unwritable");
    let dir = StorageDir::default_in(&home);
    let keychain = |_: &StorageDir| Ok(Some(stored().to_string()));

    let no_account = |_: &str| CommandOutcome::Exited {
        success: false,
        stdout: String::new(),
        stderr: "The specified item could not be found in the keychain.".to_string(),
    };

    let (result, sent) = with_test_runner(no_account, || {
        renew(
            &dir,
            &keychain,
            NOW_MS,
            &refused_url(),
            &RefusedLogin::new(),
        )
    });
    let Err(RenewError::Unavailable(why)) = result else {
        panic!("expected the renewal to be unavailable, got {result:?}");
    };
    assert!(
        why.contains("Keychain entry"),
        "refused before any grant: {why}"
    );
    assert!(
        sent.iter()
            .all(|command| command.starts_with("find-generic-password")),
        "nothing was written: {sent:?}"
    );
    let (lookup, _) = with_test_runner(no_account, || {
        current_login(&dir, &keychain, NOW_MS, &refused_url())
    });
    assert_eq!(
        lookup,
        CredentialLookup::Expired { renewable: true },
        "the login is fine; the write is on-n-off's problem, and the user's remedy is unchanged"
    );
}

#[test]
fn a_renewal_that_cannot_read_the_keychain_redeems_nothing() {
    let home = home_with("renew-keychain-unread", &stored());

    let refused = renew(
        &StorageDir::default_in(&home),
        &|_: &StorageDir| Err("Keychain lookup failed (User canceled the operation.)".to_string()),
        NOW_MS,
        &refused_url(),
        &RefusedLogin::new(),
    );
    let Err(RenewError::Unavailable(why)) = refused else {
        panic!("expected the renewal to be unavailable, got {refused:?}");
    };
    assert!(
        why.contains("User canceled"),
        "refused over the Keychain before any grant, not by the issuer: {why}"
    );
    assert_eq!(stored_token(&home), "old");
}

/// Claude Code takes `.storage-write.lock` around every change to its credentials. While another
/// process holds it, the renewal yields before redeeming anything, and breaks it only once it has
/// gone fifteen seconds without a touch.
#[test]
fn a_renewal_yields_while_claude_code_writes_its_credentials() {
    let home = home_with("renew-storage-write", &stored());
    let lock = home.join(".claude").join(".storage-write.lock");
    fs::create_dir(&lock).unwrap();
    let attempt = || {
        renew(
            &StorageDir::default_in(&home),
            &|_: &StorageDir| Ok(None),
            NOW_MS,
            &refused_url(),
            &RefusedLogin::new(),
        )
    };

    filetime::set_file_mtime(
        &lock,
        filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(10)),
    )
    .unwrap();
    assert_eq!(attempt().unwrap_err(), RenewError::Busy);
    assert_eq!(stored_token(&home), "old");
    assert!(lock.is_dir(), "the holder's lock is left alone");

    filetime::set_file_mtime(
        &lock,
        filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(16)),
    )
    .unwrap();
    assert!(
        matches!(attempt(), Err(RenewError::Unavailable(why)) if why.contains("network")),
        "past fifteen seconds the lock is abandoned, and the renewal goes on to the issuer"
    );
    assert!(!lock.exists(), "and released once the renewal is done");
}

/// A login issued to another OAuth client names it, and its refresh token is redeemable only by
/// that client, so the grant sends the login's own `clientId`, as Claude Code does. A login that
/// names none gets Claude Code's.
#[test]
fn the_grant_names_the_client_the_login_was_issued_to() {
    let mut document = stored();
    document["claudeAiOauth"]["clientId"] = json!("acme-client");
    let home = home_with("renew-client", &document);
    let (token_url, request) = serve_once_capturing("200 OK", &[], REPLY_JSON);

    assert!(matches!(
        login(&home, &token_url),
        CredentialLookup::Found(_)
    ));
    let grant: Value = serde_json::from_str(&request.join().unwrap().body).unwrap();
    assert_eq!(grant["client_id"], "acme-client");

    let home = home_with("renew-default-client", &stored());
    let (token_url, request) = serve_once_capturing("200 OK", &[], REPLY_JSON);
    assert!(matches!(
        login(&home, &token_url),
        CredentialLookup::Found(_)
    ));
    let grant: Value = serde_json::from_str(&request.join().unwrap().body).unwrap();
    assert_eq!(grant["client_id"], CLIENT_ID);
}

/// Under `CLAUDE_CONFIG_DIR` the renewal reads, locks and writes that config dir, not `~/.claude`.
#[test]
fn a_renewal_under_claude_config_dir_works_in_that_dir() {
    let home = scratch_dir("renew-config-dir");
    let work = home.join("work");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join(".credentials.json"), stored().to_string()).unwrap();
    let work_var = work.clone().into_os_string();
    let dir = claude_store::dirs(&home, &|name| {
        (name == "CLAUDE_CONFIG_DIR").then(|| work_var.clone())
    })
    .unwrap()
    .storage();
    let (token_url, request) = serve_once_capturing("200 OK", &[], REPLY_JSON);

    let renewed = renew(
        &dir,
        &|_: &StorageDir| Ok(None),
        NOW_MS,
        &token_url,
        &RefusedLogin::new(),
    );
    assert_eq!(
        renewed.map(|credential| credential.token),
        Ok("new".to_string()),
        "the login in CLAUDE_CONFIG_DIR is the one renewed"
    );
    request.join().unwrap();
    let written: Value =
        serde_json::from_str(&fs::read_to_string(work.join(".credentials.json")).unwrap()).unwrap();
    assert_eq!(written["claudeAiOauth"]["accessToken"], "new");
    assert!(!home.join(".claude").exists(), "~/.claude is never touched");
    assert!(!work.join(".oauth_refresh.lock").exists() && !home.join("work.lock").exists());
}

/// Under `CLAUDE_SECURESTORAGE_CONFIG_DIR` the renewal reads, locks and writes that storage dir.
#[test]
fn a_renewal_under_a_secure_storage_dir_works_in_that_dir() {
    let home = scratch_dir("renew-secure-storage");
    let secure = home.join("secure");
    fs::create_dir_all(&secure).unwrap();
    fs::write(secure.join(".credentials.json"), stored().to_string()).unwrap();
    let secure_var = secure.clone().into_os_string();
    let dir = claude_store::dirs(&home, &|name| {
        (name == "CLAUDE_SECURESTORAGE_CONFIG_DIR").then(|| secure_var.clone())
    })
    .unwrap()
    .storage();
    let (token_url, request) = serve_once_capturing("200 OK", &[], REPLY_JSON);

    let renewed = renew(
        &dir,
        &|_: &StorageDir| Ok(None),
        NOW_MS,
        &token_url,
        &RefusedLogin::new(),
    );
    assert_eq!(
        renewed.map(|credential| credential.token),
        Ok("new".to_string()),
        "the login in the secure storage dir is the one renewed"
    );
    request.join().unwrap();
    let written: Value =
        serde_json::from_str(&fs::read_to_string(secure.join(".credentials.json")).unwrap())
            .unwrap();
    assert_eq!(written["claudeAiOauth"]["accessToken"], "new");
    assert!(!home.join(".claude").join(".credentials.json").exists());
}

/// A refresh lock taken away while the renewal holds it means another process judged it abandoned
/// and may be renewing the same login. The heartbeat notices, and the grant is then not sent.
#[test]
fn a_renewal_whose_lock_was_taken_away_sends_no_grant() {
    let home = home_with("renew-lock-lost", &stored());
    let dir = StorageDir::default_in(&home);
    let refresh = home.join(".claude").join(".oauth_refresh.lock");
    let legacy = home.join(".claude.lock");
    // The probe runs under the locks: here another process breaks the legacy one. The heartbeat
    // touches the refresh lock and then fails on the legacy lock in the same pass, so a refresh
    // lock that is fresh again means the loss has been seen.
    let broken_under_us = |_: &StorageDir| {
        let then = SystemTime::now() - Duration::from_secs(3600);
        filetime::set_file_mtime(&refresh, filetime::FileTime::from_system_time(then)).unwrap();
        fs::remove_dir(&legacy).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while fs::metadata(&refresh)
            .and_then(|meta| meta.modified())
            .is_ok_and(|at| at.elapsed().unwrap_or_default() > Duration::from_secs(60))
        {
            assert!(
                std::time::Instant::now() < deadline,
                "no heartbeat passed while the renewal held its locks"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(200));
        Ok(None)
    };

    let result = renew(
        &dir,
        &broken_under_us,
        NOW_MS,
        &refused_url(),
        &RefusedLogin::new(),
    );
    assert_eq!(
        result,
        Err(RenewError::Busy),
        "a refused endpoint would have reported the network, so no grant was sent"
    );
    assert_eq!(stored_token(&home), "old");
}

/// The renewal reads the login it will write back while holding Claude Code's storage-write lock,
/// so no credential write of Claude Code's can land between that read and the write.
#[test]
fn a_renewal_reads_the_store_under_the_storage_write_lock() {
    let home = home_with("renew-read-locked", &stored());
    let lock = home.join(".claude").join(".storage-write.lock");
    let seen = std::cell::Cell::new(None);
    let probe = |_: &StorageDir| {
        seen.set(Some(lock.is_dir()));
        Ok(None)
    };

    let _ = renew(
        &StorageDir::default_in(&home),
        &probe,
        NOW_MS,
        &refused_url(),
        &RefusedLogin::new(),
    );
    assert_eq!(seen.get(), Some(true));
}

/// Hex, as `security -X` takes it, back to bytes.
#[cfg(target_os = "macos")]
fn unhex(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
        .collect()
}

/// A login in a scoped Keychain entry is renewed back into that entry — the scoped service, under
/// Claude Code's own account — and nowhere else.
#[cfg(target_os = "macos")]
#[test]
fn a_renewal_writes_back_to_the_scoped_keychain_entry_it_read() {
    use crate::accounts::keychain::{fake_items, with_test_runner};
    let home = scratch_dir("renew-scoped-keychain");
    let work = home.join("work");
    let dir = StorageDir::new(work.clone(), true);
    let hash = crate::sha::sha256_hex(work.to_str().unwrap().as_bytes());
    let service = format!("Claude Code-credentials-{}", &hash[..8]);
    let keychain = |_: &StorageDir| Ok(Some(stored().to_string()));
    let (token_url, request) = serve_once_capturing("200 OK", &[], REPLY_JSON);

    let (renewed, sent) = with_test_runner(fake_items(&[("claude-code-user", "{}")]), || {
        renew(&dir, &keychain, NOW_MS, &token_url, &RefusedLogin::new())
    });
    request.join().unwrap();
    assert_eq!(renewed.map(|credential| credential.token), Ok("new".into()));

    let writes: Vec<&String> = sent
        .iter()
        .filter(|command| !command.starts_with("find-generic-password"))
        .collect();
    assert_eq!(writes.len(), 1, "one write: {sent:?}");
    let prefix = format!("add-generic-password -U -a \"claude-code-user\" -s \"{service}\" -X \"");
    let hex = writes[0]
        .strip_prefix(&prefix)
        .unwrap_or_else(|| panic!("{} is not a write to {service}", writes[0]))
        .trim_end()
        .trim_end_matches('"');
    let written: Value = serde_json::from_slice(&unhex(hex)).unwrap();
    assert_eq!(written["claudeAiOauth"]["accessToken"], "new");
    assert!(
        !work.join(".credentials.json").exists(),
        "no credentials file is created"
    );
}

/// The refresh lock and the storage-write lock are both still held when the grant reaches the
/// issuer: nothing between the last check and the redemption lets either go.
#[test]
fn the_locks_are_held_while_the_grant_is_in_flight() {
    let home = home_with("renew-held-at-grant", &stored());
    let claude = home.join(".claude");
    let (token_url, request) = serve_once_observing("200 OK", REPLY_JSON, move || {
        (
            claude.join(".oauth_refresh.lock").is_dir(),
            claude.join(".storage-write.lock").is_dir(),
        )
    });

    assert!(matches!(
        login(&home, &token_url),
        CredentialLookup::Found(_)
    ));
    let (_, held) = request.join().unwrap();
    assert_eq!(held, (true, true), "(refresh lock, storage-write lock)");
}
