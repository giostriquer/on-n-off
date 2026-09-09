use super::*;
use crate::paths::scratch_dir;
use serde_json::json;

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

#[test]
fn a_renewal_replaces_the_tokens_and_leaves_everything_else_alone() {
    let mut document = stored();
    let credential = apply(&mut document, &reply(), NOW_MS).unwrap();

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

/// Losing the race is the lock working. The winner has already written a good token, and
/// redeeming a second one would spend a refresh token for nothing.
#[test]
fn a_login_renewed_by_someone_else_while_we_waited_is_used_as_it_stands() {
    let oauth = json!({"accessToken": "theirs", "expiresAt": NOW_MS + 1, "refreshToken": "rt"});
    assert_eq!(unexpired(&oauth, NOW_MS).unwrap().token, "theirs");

    let stale = json!({"accessToken": "theirs", "expiresAt": NOW_MS});
    assert!(unexpired(&stale, NOW_MS).is_none());
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

/// Claude Code files the entry under `$USER` and falls back to a fixed name rather than writing
/// an entry it would not find again. An account name it would refuse is the same as no name.
#[test]
fn the_keychain_account_matches_the_one_claude_code_files_the_entry_under() {
    assert_eq!(
        keychain_account_from("giovanne.striquer"),
        "giovanne.striquer"
    );
    assert_eq!(keychain_account_from("a_b-c.1"), "a_b-c.1");
    assert_eq!(keychain_account_from(""), "claude-code-user");
    assert_eq!(keychain_account_from("has space"), "claude-code-user");
    assert_eq!(keychain_account_from("qu\"ote"), "claude-code-user");
}
