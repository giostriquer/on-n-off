//! Read-only lookup of the CLIs' stored logins. Nothing here writes to the Keychain or the
//! credential files, and nothing refreshes a token. Tokens live in memory only: for one request,
//! plus the Claude access token memoised in-process (`ClaudeLoginMemo`) so the Keychain prompt is
//! not repeated on every refetch — never the refresh token, never on disk.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::Mutex;

use serde_json::Value;

use super::json::optional_string;
use crate::dto::LimitsAccountDto;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClaudeIdentity {
    pub(super) account: LimitsAccountDto,
    pub(super) organization_id: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ClaudeCredential {
    pub token: String,
    /// `expiresAt` from Claude Code's credential JSON, epoch milliseconds.
    pub expires_at_ms: Option<i64>,
    /// A `refreshToken` is stored alongside the access token. Only its presence is read; the
    /// token itself is never loaded, and nothing here ever redeems it.
    pub has_refresh_token: bool,
    /// `refreshTokenExpiresAt`, epoch milliseconds, when the login states one.
    pub refresh_expires_at_ms: Option<i64>,
    /// `subscriptionType` ("pro", "max", ...), used as the plan label.
    pub subscription_type: Option<String>,
}

impl ClaudeCredential {
    /// Whether Claude Code can renew this login by itself: it holds a refresh token that has not
    /// passed its own expiry. An access token past `expiresAt` is then only stale, not a lost
    /// login — the next `claude` run mints a new one without any sign-in.
    fn renewable(&self, now_ms: i64) -> bool {
        self.has_refresh_token && self.refresh_expires_at_ms.is_none_or(|at| at > now_ms)
    }
}

// Manual `Debug` so a stray `{:?}` (test panic, log line) can never print a token.
impl fmt::Debug for ClaudeCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClaudeCredential")
            .field("token", &"<redacted>")
            .field("expires_at_ms", &self.expires_at_ms)
            .field("has_refresh_token", &self.has_refresh_token)
            .field("refresh_expires_at_ms", &self.refresh_expires_at_ms)
            .field("subscription_type", &self.subscription_type)
            .finish()
    }
}

/// Everything `resolve` needs to know about a provider's login state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialLookup<T> {
    Found(T),
    /// No stored login for this provider.
    Missing,
    /// A stored login whose access token has passed its own expiry. `renewable` says the CLI can
    /// mint a new one from its refresh token, so no new sign-in is needed.
    Expired {
        renewable: bool,
    },
    /// A login may exist but could not be read (Keychain denied, unreadable file).
    Unreadable(String),
}

/// Outcome of probing the macOS Keychain: `Ok(Some(json))` entry found, `Ok(None)` no entry,
/// `Err(why)` the entry could not be read (access denied, tool failure).
pub type KeychainProbe = Result<Option<String>, String>;

#[cfg(target_os = "macos")]
const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// What one credential source (Keychain entry or file) yielded.
enum Source {
    Found(ClaudeCredential),
    /// Nothing stored (or JSON without an access token).
    Absent,
    /// Something is stored but could not be read or parsed.
    Broken(String),
}

/// Keychain entry first (macOS), then `<home>/.claude/.credentials.json` (all platforms). A token
/// past its own `expiresAt` is reported as `Expired` without any network call, tagged with whether
/// the login can still renew itself.
pub fn read_claude_credential(
    home: &Path,
    keychain: KeychainProbe,
    now_ms: i64,
) -> CredentialLookup<ClaudeCredential> {
    let sources = [
        claude_from_keychain(keychain),
        claude_from_file(&home.join(".claude").join(".credentials.json")),
    ];
    let mut first_break = None;
    for source in sources {
        match source {
            Source::Found(credential) => {
                return if credential.expires_at_ms.is_some_and(|at| at <= now_ms) {
                    CredentialLookup::Expired {
                        renewable: credential.renewable(now_ms),
                    }
                } else {
                    CredentialLookup::Found(credential)
                };
            }
            Source::Absent => {}
            Source::Broken(why) => {
                first_break.get_or_insert(why);
            }
        }
    }
    match first_break {
        Some(why) => CredentialLookup::Unreadable(why),
        None => CredentialLookup::Missing,
    }
}

fn claude_from_keychain(probe: KeychainProbe) -> Source {
    match probe {
        Ok(Some(json)) => match serde_json::from_str::<Value>(&json) {
            Ok(value) => parse_claude_credential(&value).map_or(Source::Absent, Source::Found),
            Err(error) => Source::Broken(format!("Keychain entry is not valid JSON: {error}")),
        },
        Ok(None) => Source::Absent,
        Err(why) => Source::Broken(why),
    }
}

fn claude_from_file(path: &Path) -> Source {
    match read_json_file(path) {
        Ok(Some(value)) => parse_claude_credential(&value).map_or(Source::Absent, Source::Found),
        Ok(None) => Source::Absent,
        Err(why) => Source::Broken(why),
    }
}

/// `{"claudeAiOauth": {"accessToken", "expiresAt", "refreshTokenExpiresAt", "subscriptionType", ...}}`.
fn parse_claude_credential(value: &Value) -> Option<ClaudeCredential> {
    let oauth = value.get("claudeAiOauth")?;
    let token = optional_string(oauth.get("accessToken"))?;
    Some(ClaudeCredential {
        token,
        expires_at_ms: oauth.get("expiresAt").and_then(Value::as_i64),
        has_refresh_token: optional_string(oauth.get("refreshToken")).is_some(),
        refresh_expires_at_ms: oauth.get("refreshTokenExpiresAt").and_then(Value::as_i64),
        subscription_type: optional_string(oauth.get("subscriptionType")),
    })
}

/// Which Claude account the CLI is signed into, from `<home>/.claude.json`'s `oauthAccount`
/// (Claude Code rewrites it on every login). `None` when the file or the fields are absent.
pub(super) fn read_claude_identity(home: &Path) -> Option<ClaudeIdentity> {
    let value = read_json_file(&home.join(".claude.json")).ok()??;
    let account = value.get("oauthAccount")?;
    Some(ClaudeIdentity {
        account: LimitsAccountDto {
            id: optional_string(account.get("accountUuid"))?,
            label: optional_string(account.get("emailAddress")),
        },
        organization_id: optional_string(account.get("organizationUuid")),
    })
}

#[cfg(test)]
pub fn read_claude_account(home: &Path) -> Option<LimitsAccountDto> {
    read_claude_identity(home).map(|identity| identity.account)
}

/// Process-wide memo of the last good Claude login, keyed by the account it belongs to, so a
/// user who clicked "Allow" (not "Always Allow") on the Keychain prompt sees it once per app run
/// rather than on every refetch. A hit needs the same signed-in account and an unexpired token;
/// `force` (an explicit refresh) always re-reads, and the endpoint rejecting the token clears it.
pub struct ClaudeLoginMemo(Mutex<Option<(String, ClaudeCredential)>>);

impl ClaudeLoginMemo {
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub fn lookup(
        &self,
        force: bool,
        account_id: &str,
        now_ms: i64,
        read: impl FnOnce() -> CredentialLookup<ClaudeCredential>,
    ) -> CredentialLookup<ClaudeCredential> {
        if !force {
            if let Some((memo_account, credential)) = self.slot().clone() {
                let expired = credential.expires_at_ms.is_some_and(|at| at <= now_ms);
                if memo_account == account_id && !expired {
                    return CredentialLookup::Found(credential);
                }
            }
        }
        let result = read();
        *self.slot() = match &result {
            CredentialLookup::Found(credential) => {
                Some((account_id.to_string(), credential.clone()))
            }
            _ => None,
        };
        result
    }

    pub fn clear(&self) {
        *self.slot() = None;
    }

    fn slot(&self) -> std::sync::MutexGuard<'_, Option<(String, ClaudeCredential)>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub static CLAUDE_LOGIN: ClaudeLoginMemo = ClaudeLoginMemo::new();

/// `Ok(None)` when the file does not exist; `Err` for any other I/O or JSON failure.
fn read_json_file(path: &Path) -> Result<Option<Value>, String> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<Value>(&raw)
            .map(Some)
            .map_err(|error| format!("{}: {error}", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Map `/usr/bin/security find-generic-password -w` output to a probe result. Kept pure so the
/// branching is testable on every platform; only the spawn below is macOS-specific.
#[cfg(any(target_os = "macos", test))]
fn interpret_security_outcome(success: bool, stdout: &str, stderr: &str) -> KeychainProbe {
    if success {
        let json = stdout.trim();
        return Ok((!json.is_empty()).then(|| json.to_string()));
    }
    let detail = stderr.trim();
    if detail.contains("could not be found") {
        return Ok(None);
    }
    let detail = if detail.is_empty() {
        "no details".to_string()
    } else {
        detail.to_string()
    };
    Err(format!(
        "Keychain lookup failed ({detail}). If macOS asked to allow access, click Allow and refresh."
    ))
}

/// Probe the macOS Keychain for Claude Code's login. The first call shows the system "allow
/// access" dialog, so the deadline is generous; on timeout the tool is killed.
#[cfg(target_os = "macos")]
pub fn keychain_claude_json() -> KeychainProbe {
    use crate::process::{wait_with_deadline, CommandOutcome};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    let child = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", CLAUDE_KEYCHAIN_SERVICE, "-w"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not run /usr/bin/security: {error}"))?;
    match wait_with_deadline(child, Duration::from_secs(90)) {
        Ok(CommandOutcome::Exited {
            success,
            stdout,
            stderr,
        }) => interpret_security_outcome(success, &stdout, &stderr),
        Ok(CommandOutcome::TimedOut) => Err("Keychain prompt was not answered in time".to_string()),
        Err(error) => Err(format!("Keychain lookup failed: {error}")),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn keychain_claude_json() -> KeychainProbe {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::scratch_dir;
    use std::fs;

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
        let found = |token: &str| CredentialLookup::Found(login(token, Some(2_000)));
        assert_eq!(memo.lookup(false, "acct", 1_000, read), found("t1"));
        assert_eq!(memo.lookup(false, "acct", 1_000, read), found("t1"));
        assert_eq!(reads.get(), 1);
        // Explicit refresh re-reads.
        assert_eq!(memo.lookup(true, "acct", 1_000, read), found("t2"));
        // Past the memoised expiry the memo is a miss (the CLI may have refreshed since).
        assert_eq!(memo.lookup(false, "acct", 2_000, read), found("t3"));
        // A different signed-in account never gets the other account's token.
        assert_eq!(memo.lookup(false, "other", 1_000, read), found("t4"));
        memo.clear();
        assert_eq!(memo.lookup(false, "other", 1_000, read), found("t5"));
        assert_eq!(reads.get(), 5);
    }

    #[test]
    fn login_memo_does_not_remember_misses() {
        let memo = ClaudeLoginMemo::new();
        assert_eq!(
            memo.lookup(false, "acct", 0, || CredentialLookup::Missing),
            CredentialLookup::Missing
        );
        assert_eq!(
            memo.lookup(false, "acct", 0, || CredentialLookup::Found(login(
                "t", None
            ))),
            CredentialLookup::Found(login("t", None))
        );
        // A forced miss evicts the memo instead of serving the old token next time.
        assert_eq!(
            memo.lookup(true, "acct", 0, || CredentialLookup::Expired {
                renewable: true
            }),
            CredentialLookup::Expired { renewable: true }
        );
        assert_eq!(
            memo.lookup(false, "acct", 0, || CredentialLookup::Missing),
            CredentialLookup::Missing
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
}
