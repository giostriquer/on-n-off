//! Borrow the GitHub CLI's login: `gh auth token` prints the token `gh` holds for github.com.
//! on-n-off never stores it — it lives in memory only, memoised for the app run so `gh` (and its
//! Keychain lookup) runs once, and dropped the moment GitHub rejects it.

use std::fmt;
use std::process::Stdio;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use crate::cli::AgentCli;
use crate::process::{wait_with_deadline, CommandOutcome};

pub(super) const TOKEN_DEADLINE: Duration = Duration::from_secs(10);
const GH_HOST: &str = "github.com";

#[derive(Clone, PartialEq, Eq)]
pub struct GhToken(String);

impl GhToken {
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

// Manual `Debug` so a stray `{:?}` (test panic, log line) can never print the token.
impl fmt::Debug for GhToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GhToken(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TokenError {
    /// `gh` is not on the CLI search path.
    GhMissing,
    /// `gh` ran but holds no github.com login (non-zero exit, or nothing token-shaped printed).
    /// Its stderr is deliberately not carried: it can name files and accounts.
    NotLoggedIn,
    /// `gh auth token` did not answer within the deadline (a stuck credential helper, say).
    TimedOut,
    /// `gh` could not be started for another reason (permissions, a broken launcher).
    Spawn(String),
}

/// Ask `gh` for its github.com token. `cli` is injected so tests can point at a stub launcher.
pub(super) fn read_token_with(cli: &AgentCli, deadline: Duration) -> Result<GhToken, TokenError> {
    let child = cli
        .command()
        .args(["auth", "token", "--hostname", GH_HOST])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => TokenError::GhMissing,
            _ => TokenError::Spawn(error.to_string()),
        })?;
    match wait_with_deadline(child, deadline)
        .map_err(|error| TokenError::Spawn(error.to_string()))?
    {
        CommandOutcome::Exited {
            success: true,
            stdout,
            ..
        } => {
            let token = stdout.trim();
            if token.is_empty() || token.contains(char::is_whitespace) {
                Err(TokenError::NotLoggedIn)
            } else {
                Ok(GhToken(token.to_string()))
            }
        }
        CommandOutcome::Exited { .. } => Err(TokenError::NotLoggedIn),
        CommandOutcome::TimedOut => Err(TokenError::TimedOut),
    }
}

/// Process-wide memo of the last token `gh` handed over. A miss re-runs `gh`; failures are never
/// kept, and `clear` (on a 401 from GitHub) forces the next lookup to ask `gh` again.
pub struct GhTokenMemo(Mutex<Option<GhToken>>);

impl GhTokenMemo {
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub(super) fn lookup(
        &self,
        read: impl FnOnce() -> Result<GhToken, TokenError>,
    ) -> Result<GhToken, TokenError> {
        if let Some(token) = self.slot().clone() {
            return Ok(token);
        }
        let result = read();
        *self.slot() = result.as_ref().ok().cloned();
        result
    }

    pub(super) fn clear(&self) {
        *self.slot() = None;
    }

    fn slot(&self) -> MutexGuard<'_, Option<GhToken>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub static GH_TOKEN: GhTokenMemo = GhTokenMemo::new();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::AgentCli;
    use crate::cli_stub::CliStub;
    use crate::paths::scratch_dir;
    use std::cell::Cell;
    use std::fs;
    use std::time::{Duration, Instant};

    #[test]
    fn a_logged_in_gh_hands_over_its_trimmed_token() {
        let dir = scratch_dir("gh-token-ok");
        let cli = CliStub::new("gh")
            .log_args("args.txt", false)
            .stdout("gho_abc123")
            .cli(&dir);
        let token = read_token_with(&cli, TOKEN_DEADLINE).unwrap();
        assert_eq!(token.as_str(), "gho_abc123");
        let args = fs::read_to_string(dir.join("args.txt")).unwrap();
        assert!(args.contains("auth token"), "{args}");
        assert!(args.contains("--hostname github.com"), "{args}");
    }

    #[test]
    fn a_signed_out_gh_is_not_logged_in_and_its_stderr_stays_private() {
        let dir = scratch_dir("gh-token-signed-out");
        let cli = CliStub::new("gh")
            .stderr("no oauth token found for github.com: secret-path")
            .exit(1)
            .cli(&dir);
        let error = read_token_with(&cli, TOKEN_DEADLINE).unwrap_err();
        assert_eq!(error, TokenError::NotLoggedIn);
        assert!(!format!("{error:?}").contains("secret-path"));
    }

    #[test]
    fn an_empty_or_multi_word_answer_is_not_a_token() {
        let dir = scratch_dir("gh-token-empty");
        let cli = CliStub::new("gh").stdout("").cli(&dir);
        assert_eq!(
            read_token_with(&cli, TOKEN_DEADLINE).unwrap_err(),
            TokenError::NotLoggedIn
        );
        let cli = CliStub::new("gh").stdout("not a token").cli(&dir.join("b"));
        assert_eq!(
            read_token_with(&cli, TOKEN_DEADLINE).unwrap_err(),
            TokenError::NotLoggedIn
        );
    }

    #[test]
    fn a_missing_gh_is_reported_as_missing() {
        let dir = scratch_dir("gh-token-missing");
        let cli = AgentCli::new(dir.join("gh").to_string_lossy().as_ref());
        assert_eq!(
            read_token_with(&cli, TOKEN_DEADLINE).unwrap_err(),
            TokenError::GhMissing
        );
    }

    #[test]
    fn a_hung_gh_is_killed_at_the_deadline() {
        let dir = scratch_dir("gh-token-hang");
        let cli = CliStub::new("gh").sleep(5).cli(&dir);
        let started = Instant::now();
        assert_eq!(
            read_token_with(&cli, Duration::from_millis(100)).unwrap_err(),
            TokenError::TimedOut
        );
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn the_memo_reads_once_until_cleared() {
        let memo = GhTokenMemo::new();
        let reads = Cell::new(0_u32);
        let read = || {
            reads.set(reads.get() + 1);
            Ok(GhToken("gho_memo".into()))
        };
        assert_eq!(memo.lookup(read).unwrap().as_str(), "gho_memo");
        assert_eq!(memo.lookup(read).unwrap().as_str(), "gho_memo");
        assert_eq!(reads.get(), 1);
        memo.clear();
        assert_eq!(memo.lookup(read).unwrap().as_str(), "gho_memo");
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn the_memo_never_keeps_a_failure() {
        let memo = GhTokenMemo::new();
        let reads = Cell::new(0_u32);
        let read = || {
            reads.set(reads.get() + 1);
            Err(TokenError::NotLoggedIn)
        };
        assert_eq!(memo.lookup(read).unwrap_err(), TokenError::NotLoggedIn);
        assert_eq!(memo.lookup(read).unwrap_err(), TokenError::NotLoggedIn);
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn debug_output_never_contains_the_token() {
        let token = GhToken("gho_very_secret".into());
        let printed = format!("{token:?} {:?}", Ok::<_, TokenError>(token.clone()));
        assert!(!printed.contains("very_secret"), "{printed}");
        assert!(printed.contains("redacted"), "{printed}");
    }
}
