use super::*;
use crate::paths::scratch_dir;
use std::fs;

pub fn read_claude_account(home: &Path) -> Option<LimitsAccountDto> {
    read_claude_identity(home).map(|identity| identity.account)
}

const CLAUDE_JSON: &str = r#"{"claudeAiOauth":{"accessToken":"kc-token","refreshToken":"r","expiresAt":1787022473402,"scopes":["user:inference"],"subscriptionType":"max","rateLimitTier":"default_claude_max_5x"}}"#;
/// Well before the fixture's `expiresAt`.
const NOW_MS: i64 = 1787000000000;

fn write(home: &std::path::Path, rel: &str, body: &str) {
    let path = home.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn claude_prefers_the_keychain_entry_over_the_credentials_file() {
    let home = scratch_dir("limits-cred");
    write(
        &home,
        ".claude/.credentials.json",
        &CLAUDE_JSON.replace("kc-token", "file-token"),
    );
    match read_claude_credential(&home, Ok(Some(CLAUDE_JSON.to_string())), NOW_MS) {
        CredentialLookup::Found(cred) => {
            assert_eq!(cred.token, "kc-token");
            assert_eq!(cred.expires_at_ms, Some(1787022473402));
            assert_eq!(cred.subscription_type.as_deref(), Some("max"));
        }
        other => panic!("expected Found, got {other:?}"),
    }
}

#[test]
fn claude_falls_back_to_the_credentials_file_when_the_keychain_has_no_entry() {
    let home = scratch_dir("limits-cred");
    write(
        &home,
        ".claude/.credentials.json",
        &CLAUDE_JSON.replace("kc-token", "file-token"),
    );
    match read_claude_credential(&home, Ok(None), NOW_MS) {
        CredentialLookup::Found(cred) => assert_eq!(cred.token, "file-token"),
        other => panic!("expected Found, got {other:?}"),
    }
}

#[test]
fn claude_without_any_stored_login_is_missing() {
    let home = scratch_dir("limits-cred");
    assert_eq!(
        read_claude_credential(&home, Ok(None), NOW_MS),
        CredentialLookup::Missing
    );
}

#[test]
fn claude_token_past_its_expiry_is_expired_from_either_source() {
    let home = scratch_dir("limits-cred");
    let after = 1787022473402 + 1;
    assert_eq!(
        read_claude_credential(&home, Ok(Some(CLAUDE_JSON.to_string())), after),
        CredentialLookup::Expired { renewable: true }
    );
    write(&home, ".claude/.credentials.json", CLAUDE_JSON);
    assert_eq!(
        read_claude_credential(&home, Ok(None), after),
        CredentialLookup::Expired { renewable: true }
    );
    // A credential without `expiresAt` is trusted; the endpoint decides.
    let no_expiry = CLAUDE_JSON.replace(r#""expiresAt":1787022473402,"#, "");
    assert!(matches!(
        read_claude_credential(&home, Ok(Some(no_expiry)), after),
        CredentialLookup::Found(_)
    ));
}

/// The refresh token decides whether an expired access token needs a new sign-in: still
/// valid, the CLI renews it; gone or itself expired, only signing in again helps.
#[test]
fn an_expired_claude_token_is_renewable_only_while_its_refresh_token_lives() {
    let home = scratch_dir("limits-cred");
    let after = 1787022473402 + 1;
    let dated = CLAUDE_JSON.replace(
        r#""refreshToken":"r","#,
        &format!(r#""refreshToken":"r","refreshTokenExpiresAt":{after},"#),
    );
    assert_eq!(
        read_claude_credential(&home, Ok(Some(dated)), after),
        CredentialLookup::Expired { renewable: false },
        "a refresh token at its own expiry cannot renew anything"
    );
    let no_refresh = CLAUDE_JSON.replace(r#""refreshToken":"r","#, "");
    assert_eq!(
        read_claude_credential(&home, Ok(Some(no_refresh)), after),
        CredentialLookup::Expired { renewable: false }
    );
}

#[test]
fn claude_keychain_denial_is_unreadable_unless_the_file_covers_it() {
    let home = scratch_dir("limits-cred");
    match read_claude_credential(&home, Err("Keychain access denied".to_string()), NOW_MS) {
        CredentialLookup::Unreadable(message) => {
            assert!(message.contains("Keychain access denied"))
        }
        other => panic!("expected Unreadable, got {other:?}"),
    }
    write(&home, ".claude/.credentials.json", CLAUDE_JSON);
    assert!(matches!(
        read_claude_credential(&home, Err("denied".to_string()), NOW_MS),
        CredentialLookup::Found(_)
    ));
}

#[test]
fn claude_credentials_without_an_access_token_count_as_signed_out_from_either_source() {
    let home = scratch_dir("limits-cred");
    let no_token = r#"{"claudeAiOauth":{"expiresAt":1}}"#;
    assert_eq!(
        read_claude_credential(&home, Ok(Some(no_token.to_string())), NOW_MS),
        CredentialLookup::Missing
    );
    write(&home, ".claude/.credentials.json", no_token);
    assert_eq!(
        read_claude_credential(&home, Ok(None), NOW_MS),
        CredentialLookup::Missing
    );
}

#[test]
fn claude_malformed_json_is_unreadable_from_either_source() {
    let home = scratch_dir("limits-cred");
    assert!(matches!(
        read_claude_credential(&home, Ok(Some("{not json".to_string())), NOW_MS),
        CredentialLookup::Unreadable(_)
    ));
    write(&home, ".claude/.credentials.json", "{not json");
    assert!(matches!(
        read_claude_credential(&home, Ok(None), NOW_MS),
        CredentialLookup::Unreadable(_)
    ));
}

#[test]
fn claude_account_comes_from_claude_json_oauth_account() {
    let home = scratch_dir("limits-cred");
    assert_eq!(read_claude_account(&home), None);
    write(
        &home,
        ".claude.json",
        r#"{"oauthAccount":{"accountUuid":"uuid-1","emailAddress":"me@example.com","organizationName":"Org","organizationUuid":"org-1"},"userID":"x"}"#,
    );
    assert_eq!(
        read_claude_identity(&home),
        Some(ClaudeIdentity {
            account: LimitsAccountDto {
                id: "uuid-1".to_string(),
                label: Some("me@example.com".to_string())
            },
            organization_id: Some("org-1".to_string())
        })
    );
    assert_eq!(
        read_claude_account(&home),
        Some(LimitsAccountDto {
            id: "uuid-1".to_string(),
            label: Some("me@example.com".to_string())
        })
    );
    write(
        &home,
        ".claude.json",
        r#"{"oauthAccount":{"emailAddress":"no-uuid@example.com"}}"#,
    );
    assert_eq!(read_claude_account(&home), None);
    write(&home, ".claude.json", "{broken");
    assert_eq!(read_claude_account(&home), None);
}

fn login(token: &str, expires_at_ms: Option<i64>) -> ClaudeCredential {
    ClaudeCredential {
        token: token.to_string(),
        expires_at_ms,
        has_refresh_token: true,
        refresh_expires_at_ms: None,
        subscription_type: Some("max".to_string()),
    }
}

#[test]
fn login_memo_serves_the_same_account_until_forced_expired_or_cleared() {
    let memo = ClaudeLoginMemo::new();
    let reads = std::cell::Cell::new(0);
    let read = || {
        reads.set(reads.get() + 1);
        CredentialLookup::Found(login(&format!("t{}", reads.get()), Some(2_000)))
    };
    let stored = |token: &str| {
        (
            CredentialLookup::Found(login(token, Some(2_000))),
            LoginSource::Stored,
        )
    };
    let memoised = |token: &str| {
        (
            CredentialLookup::Found(login(token, Some(2_000))),
            LoginSource::Memo,
        )
    };
    assert_eq!(memo.lookup(false, "acct", 1_000, read), stored("t1"));
    assert_eq!(
        memo.lookup(false, "acct", 1_000, read),
        memoised("t1"),
        "a hit says so, because a rejected memo is worth retrying and a rejected stored login is not"
    );
    assert_eq!(reads.get(), 1);
    // Explicit refresh re-reads.
    assert_eq!(memo.lookup(true, "acct", 1_000, read), stored("t2"));
    // Past the memoised expiry the memo is a miss (the CLI may have refreshed since).
    assert_eq!(memo.lookup(false, "acct", 2_000, read), stored("t3"));
    // A different signed-in account never gets the other account's token.
    assert_eq!(memo.lookup(false, "other", 1_000, read), stored("t4"));
    memo.clear();
    assert_eq!(memo.lookup(false, "other", 1_000, read), stored("t5"));
    assert_eq!(reads.get(), 5);
}

#[test]
fn login_memo_does_not_remember_misses() {
    let memo = ClaudeLoginMemo::new();
    assert_eq!(
        memo.lookup(false, "acct", 0, || CredentialLookup::Missing),
        (CredentialLookup::Missing, LoginSource::Stored)
    );
    assert_eq!(
        memo.lookup(false, "acct", 0, || CredentialLookup::Found(login(
            "t", None
        ))),
        (
            CredentialLookup::Found(login("t", None)),
            LoginSource::Stored
        )
    );
    // A forced miss evicts the memo instead of serving the old token next time.
    assert_eq!(
        memo.lookup(true, "acct", 0, || CredentialLookup::Expired {
            renewable: true
        }),
        (
            CredentialLookup::Expired { renewable: true },
            LoginSource::Stored
        )
    );
    assert_eq!(
        memo.lookup(false, "acct", 0, || CredentialLookup::Missing),
        (CredentialLookup::Missing, LoginSource::Stored)
    );
}

#[test]
fn debug_output_never_contains_the_token() {
    let claude = ClaudeCredential {
        token: "secret-claude".to_string(),
        expires_at_ms: None,
        has_refresh_token: true,
        refresh_expires_at_ms: None,
        subscription_type: Some("max".to_string()),
    };
    let printed = format!("{claude:?}");
    assert!(!printed.contains("secret-"), "{printed}");
    assert!(printed.contains("<redacted>"));
}

#[test]
fn security_tool_outcomes_map_to_probe_results() {
    assert_eq!(
        interpret_security_outcome(true, "  {\"a\":1}\n", ""),
        Ok(Some("{\"a\":1}".to_string()))
    );
    assert_eq!(interpret_security_outcome(true, "\n", ""), Ok(None));
    assert_eq!(
        interpret_security_outcome(
            false,
            "",
            "security: SecKeychainSearchCopyNext: The specified item could not be found in the keychain."
        ),
        Ok(None)
    );
    let denied = interpret_security_outcome(
        false,
        "",
        "security: SecKeychainItemCopyContent: User canceled the operation.",
    )
    .unwrap_err();
    assert!(denied.contains("User canceled"), "{denied}");
    assert!(denied.contains("click Allow"), "{denied}");
}

/// The renewal writes back to the store this reports, so which one it picks and when it falls
/// through are the same question the read answers — and it has to stay one answer. A Keychain
/// entry that holds no usable login must not shadow a file that does.
#[test]
fn the_login_document_names_the_store_it_came_from_and_falls_through_like_the_read() {
    let home = scratch_dir("limits-store");
    let file_json = CLAUDE_JSON.replace("kc-token", "file-token");
    write(&home, ".claude/.credentials.json", &file_json);
    let path = home.join(".claude").join(".credentials.json");

    let (store, document) = claude_login_document(&home, Ok(Some(CLAUDE_JSON.to_string())))
        .unwrap()
        .unwrap();
    assert_eq!(store, ClaudeStore::Keychain);
    assert_eq!(document["claudeAiOauth"]["accessToken"], "kc-token");

    let (store, document) = claude_login_document(&home, Ok(None)).unwrap().unwrap();
    assert_eq!(store, ClaudeStore::File(path.clone()));
    assert_eq!(document["claudeAiOauth"]["accessToken"], "file-token");

    // JSON without a login, and JSON that will not parse: both let the file behind have its turn,
    // exactly as `read_claude_credential` does.
    for shadow in [r#"{"claudeAiOauth":{}}"#, "{ not json"] {
        let (store, _) = claude_login_document(&home, Ok(Some(shadow.to_string())))
            .unwrap()
            .unwrap();
        assert_eq!(
            store,
            ClaudeStore::File(path.clone()),
            "a useless Keychain entry must not shadow a usable file: {shadow}"
        );
    }
}

/// The two agree by construction, and this is what says so: whatever store the document came from,
/// the credential the read hands out is the one parsed from that same document.
#[test]
fn the_read_and_the_document_lookup_never_disagree_about_the_login() {
    let home = scratch_dir("limits-store-agree");
    write(&home, ".claude/.credentials.json", CLAUDE_JSON);
    for probe in [
        Ok(None),
        Ok(Some(CLAUDE_JSON.replace("kc-token", "other").to_string())),
        Ok(Some("{ not json".to_string())),
    ] {
        let document = claude_login_document(&home, probe.clone())
            .unwrap()
            .map(|(_, document)| document);
        let expected = document.as_ref().and_then(parse_claude_credential);
        match read_claude_credential(&home, probe, NOW_MS) {
            CredentialLookup::Found(credential) => assert_eq!(Some(credential), expected),
            other => panic!("expected a login, got {other:?}"),
        }
    }
}

/// A guessed account name would file a second Keychain item under the same service, and the read
/// matches on service alone, so it could then return either one.
#[test]
fn the_keychain_account_is_read_off_the_entry_rather_than_guessed() {
    let dump = "keychain: \"/Users/me/Library/Keychains/login.keychain-db\"\n\
        attributes:\n    \
        \"acct\"<blob>=\"giovanne.striquer\"\n    \
        \"svce\"<blob>=\"Claude Code-credentials\"\n";
    assert_eq!(
        parse_keychain_account(dump).as_deref(),
        Some("giovanne.striquer")
    );
    assert_eq!(parse_keychain_account("attributes:\n"), None);
    assert_eq!(parse_keychain_account("    \"acct\"<blob>=\"\"\n"), None);
}
