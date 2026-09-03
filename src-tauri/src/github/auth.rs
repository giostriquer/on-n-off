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
mod tests;
