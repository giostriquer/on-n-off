use super::*;
use crate::http::{refused_url, serve_once_capturing};
use crate::paths::scratch_dir;
use serde_json::json;
use std::fs;

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
    current_login(home, &|| Ok(None), NOW_MS, token_url)
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
    let body = request_body("rt", &scopes(&stored()["claudeAiOauth"]));
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
    let credential = renew(&home, &|| Ok(None), NOW_MS, &refused_url()).unwrap();
    assert_eq!(credential.token, "theirs");
}

/// While Claude Code holds the lock it is renewing the same login, so the next poll reads what it
/// wrote. Nothing is redeemed here and the message the user already had stands.
#[test]
fn a_held_lock_leaves_the_renewal_to_whoever_holds_it() {
    let home = home_with("renew-busy", &stored());
    let _held = RefreshLock::acquire(&home).unwrap();

    assert_eq!(
        login(&home, &refused_url()),
        CredentialLookup::Expired { renewable: true },
        "a refused endpoint would have reported differently, so nothing was sent"
    );
}

/// `invalid_grant`: the refresh token is spent or revoked, and no amount of running `claude` will
/// renew it. The user has to sign in again and the message has to say so.
#[test]
fn a_refused_refresh_token_asks_for_a_new_sign_in_rather_than_a_renewal() {
    let home = home_with("renew-rejected", &stored());
    let (token_url, request) =
        serve_once_capturing("400 Bad Request", &[], r#"{"error":"invalid_grant"}"#);

    assert_eq!(
        login(&home, &token_url),
        CredentialLookup::Expired { renewable: false }
    );
    request.join().unwrap();
}

/// A store that cannot be read is a problem with the store, not with the login. Reporting it as an
/// expiry would send the user to run `claude` over a login on-n-off could not read either.
#[test]
fn a_store_that_cannot_be_read_reports_the_store() {
    let home = scratch_dir("renew-unreadable");
    let path = home.join(".claude").join(".credentials.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{ not json").unwrap();

    let CredentialLookup::Unreadable(why) = login(&home, &refused_url()) else {
        panic!("expected the store's own failure");
    };
    assert!(why.contains(".credentials.json"), "{why}");
}

/// The dangerous case: redeemed, then not stored. The refresh token that bought the new login is
/// spent, so "run `claude` to renew it" is advice that cannot work any more, and saying it would
/// leave the user hunting a problem on-n-off caused.
#[test]
fn a_renewal_that_cannot_be_stored_says_so_instead_of_offering_a_dead_remedy() {
    let home = home_with("renew-stranded", &stored());
    let (token_url, request) = serve_once_capturing("200 OK", &[], REPLY_JSON);

    // A directory sitting where the write needs its temporary file. Everything up to and including
    // the redemption works; only the store fails, which is the case worth being sure about.
    let blocked = home
        .join(".claude")
        .join(".credentials.json.on-n-off")
        .to_path_buf();
    fs::create_dir(&blocked).unwrap();

    assert_eq!(login(&home, &token_url), CredentialLookup::Stranded);
    request.join().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(
            &fs::read_to_string(home.join(".claude").join(".credentials.json")).unwrap()
        )
        .unwrap()["claudeAiOauth"]["accessToken"],
        "old",
        "nothing half-written reached the store"
    );
}

#[test]
fn the_refresh_lock_admits_one_holder_and_frees_both_paths_on_drop() {
    let home = scratch_dir("renew-lock");
    let paths = [
        home.join(".claude").join(".oauth_refresh.lock"),
        home.join(".claude.lock"),
    ];

    let held = RefreshLock::acquire(&home).unwrap();
    assert!(paths.iter().all(|path| path.is_dir()));
    assert_eq!(RefreshLock::acquire(&home).unwrap_err(), RenewError::Busy);

    drop(held);
    assert!(
        paths.iter().all(|path| !path.exists()),
        "a released lock leaves nothing behind for the next renewal to break"
    );
    RefreshLock::acquire(&home).unwrap();
}

/// A process killed mid-renewal leaves its lock directory behind. Claude Code breaks one older
/// than a minute rather than never refreshing again, and so must this.
#[test]
fn a_lock_left_behind_by_a_dead_process_is_broken_once_it_goes_stale() {
    let home = scratch_dir("renew-stale");
    let abandoned = home.join(".claude").join(".oauth_refresh.lock");
    fs::create_dir_all(&abandoned).unwrap();
    assert_eq!(RefreshLock::acquire(&home).unwrap_err(), RenewError::Busy);

    let past_stale = SystemTime::now() + LOCK_STALE + Duration::from_secs(5);
    assert!(RefreshLock::acquire_at(&home, past_stale).is_ok());
}

/// The renewed login lands in a file only this user can read, and the replacement is one rename,
/// so a reader never sees a half-written login.
#[test]
fn the_credentials_file_is_replaced_atomically_and_stays_private() {
    let home = scratch_dir("renew-file");
    let path = home.join(".claude").join(".credentials.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{}").unwrap();

    write_file(&path, r#"{"claudeAiOauth":{"accessToken":"new"}}"#).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        r#"{"claudeAiOauth":{"accessToken":"new"}}"#
    );
    assert!(
        !path.with_extension("json.on-n-off").exists(),
        "the temporary is renamed, not left beside the login"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the file holds a refresh token");
    }
}

/// The token reaches `security` on stdin, hex-encoded. Nothing about the login can then be read
/// out of the process table, and no quoting in the JSON can escape into the command.
#[test]
fn the_keychain_write_hex_encodes_the_login_instead_of_quoting_it() {
    let command = keychain_command("me", "Claude Code-credentials", r#"{"a":"b\"c"}"#);
    assert!(
        command.starts_with(r#"add-generic-password -U -a "me" -s "Claude Code-credentials" -X ""#),
        "{command}"
    );
    let hex = command
        .rsplit_once("-X \"")
        .unwrap()
        .1
        .trim_end()
        .trim_end_matches('"');
    assert!(hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(
        !command.contains(r#"b\"c"#),
        "no JSON reaches the command line"
    );
}

/// The Keychain write against a throwaway entry of our own, so the real login is never at stake:
/// `security` really runs, the command really arrives over stdin, and `-U` really replaces an
/// entry that already exists. The unit test above pins the command's shape; this pins that macOS
/// accepts it.
///
/// `cargo test --manifest-path src-tauri/Cargo.toml rehearse_the_keychain_write -- --ignored`
#[cfg(target_os = "macos")]
#[test]
#[ignore = "writes a throwaway Keychain entry; not part of CI"]
fn rehearse_the_keychain_write() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    const SERVICE: &str = "on-n-off keychain write rehearsal";
    let account = "on-n-off-test";
    let read = || {
        let output = Command::new("/usr/bin/security")
            .args(["find-generic-password", "-a", account, "-s", SERVICE, "-w"])
            .output()
            .unwrap();
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    let write = |raw: &str| {
        let mut child = Command::new("/usr/bin/security")
            .arg("-i")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(keychain_command(account, SERVICE, raw).as_bytes())
            .unwrap();
        assert!(
            child.wait().unwrap().success(),
            "security refused the write"
        );
    };

    // A quote and a space in the payload: the two things hex encoding exists to survive.
    let first = r#"{"claudeAiOauth":{"accessToken":"one","note":"a \"quoted\" word"}}"#;
    let second = r#"{"claudeAiOauth":{"accessToken":"two"}}"#;
    write(first);
    assert_eq!(
        read().as_deref(),
        Some(first),
        "the entry round-trips byte for byte"
    );
    write(second);
    assert_eq!(
        read().as_deref(),
        Some(second),
        "-U replaces an entry that already exists rather than failing or duplicating it"
    );

    Command::new("/usr/bin/security")
        .args(["delete-generic-password", "-a", account, "-s", SERVICE])
        .output()
        .unwrap();
    assert_eq!(read(), None, "the rehearsal cleans up after itself");
}
