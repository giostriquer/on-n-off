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
        &home,
        &|| Ok(None),
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
    let _held = RefreshLock::acquire(&home).unwrap();

    assert_eq!(
        renew(
            &home,
            &|| Ok(None),
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
            &home,
            &|| Ok(None),
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
/// taking both of Claude Code's lock directories every five minutes for as long as the user leaves
/// it alone. A login Claude Code has since rewritten is a different login, and worth trying.
#[test]
fn the_same_refused_login_is_not_sent_a_second_time() {
    let home = home_with("renew-refused-memo", &stored());
    let refused = RefusedLogin::new();
    let (token_url, request) = serve_once_capturing("400 Bad Request", &[], r#"{"error":"x"}"#);

    let attempt = |url: &str| renew(&home, &|| Ok(None), NOW_MS, url, &refused);
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
/// the grant is sent, because after it there is no un-renewed state to fall back to.
#[test]
fn a_write_that_cannot_be_prepared_fails_before_anything_is_spent() {
    let home = home_with("renew-unwritable", &stored());
    let path = home.join(".claude").join(".credentials.json");
    // A directory where the temporary has to go.
    fs::create_dir(path.with_extension("json.on-n-off")).unwrap();

    assert!(Writer::prepare(&ClaudeStore::File(path)).is_err());
    assert!(matches!(
        renew(
            &home,
            &|| Ok(None),
            NOW_MS,
            &refused_url(),
            &RefusedLogin::new()
        ),
        Err(RenewError::Unavailable(_))
    ));
    assert_eq!(
        login(&home, &refused_url()),
        CredentialLookup::Expired { renewable: true },
        "the login is fine; the write is on-n-off's problem, and the user's remedy is unchanged"
    );
    assert_eq!(stored_token(&home), "old");
}

/// A prepared write that is never committed takes its temporary with it. Leaving one behind would
/// park a live refresh token in a file Claude Code neither knows about nor rotates, which is the
/// same objection that keeps `ConfigIo` out of this module.
#[test]
fn an_abandoned_write_leaves_no_temporary_holding_a_token() {
    let home = home_with("renew-temp", &stored());
    let path = home.join(".claude").join(".credentials.json");
    let temporary = path.with_extension("json.on-n-off");

    let writer = Writer::prepare(&ClaudeStore::File(path)).unwrap();
    assert!(temporary.exists(), "prepared up front, before the grant");
    drop(writer);
    assert!(!temporary.exists());
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

/// The one `security` call made under the lock has to finish well inside the minute after which
/// Claude Code breaks it, or the write races whoever broke it.
#[cfg(target_os = "macos")]
#[test]
fn the_keychain_write_deadline_fits_inside_the_lock_it_is_held_under() {
    assert!(KEYCHAIN_WRITE_DEADLINE < LOCK_STALE);
}

/// The renewed login lands in a file only this user can read.
#[test]
fn the_credentials_file_is_written_private() {
    let home = scratch_dir("renew-file");
    let path = home.join(".claude").join(".credentials.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();

    write_private(&path, r#"{"claudeAiOauth":{"accessToken":"new"}}"#).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        r#"{"claudeAiOauth":{"accessToken":"new"}}"#
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
    let service = credentials::CLAUDE_KEYCHAIN_SERVICE;
    let command = keychain_command("me", service, r#"{"a":"b\"c"}"#);
    assert!(
        command.starts_with(&format!(
            r#"add-generic-password -U -a "me" -s "{service}" -X ""#
        )),
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

/// The production write, driven against a throwaway entry of our own so the real login is never at
/// stake. This calls `write_keychain` itself rather than re-implementing its spawn, so the two
/// cannot drift, and it covers the account lookup the renewal resolves before it spends anything.
///
/// `cargo test --manifest-path src-tauri/Cargo.toml rehearse_the_keychain_write -- --ignored`
#[cfg(target_os = "macos")]
#[test]
#[ignore = "writes a throwaway Keychain entry; not part of CI"]
fn rehearse_the_keychain_write() {
    use std::process::Command;

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

    // A quote and a space in the payload: the two things hex encoding exists to survive.
    let first = r#"{"claudeAiOauth":{"accessToken":"one","note":"a \"quoted\" word"}}"#;
    let second = r#"{"claudeAiOauth":{"accessToken":"two"}}"#;
    write_keychain(account, SERVICE, first).unwrap();
    assert_eq!(
        read().as_deref(),
        Some(first),
        "the entry round-trips byte for byte"
    );
    write_keychain(account, SERVICE, second).unwrap();
    assert_eq!(
        read().as_deref(),
        Some(second),
        "-U replaces an entry that already exists rather than failing or duplicating it"
    );

    // The account lookup, against output `security` really produced rather than a fixture.
    let attributes = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", SERVICE])
        .output()
        .unwrap();
    assert_eq!(
        credentials::parse_keychain_account(&String::from_utf8_lossy(&attributes.stdout))
            .as_deref(),
        Some(account),
        "the parser reads `security`'s own output, not just a hand-written fixture"
    );

    Command::new("/usr/bin/security")
        .args(["delete-generic-password", "-a", account, "-s", SERVICE])
        .output()
        .unwrap();
    assert_eq!(read(), None, "the rehearsal cleans up after itself");
}
