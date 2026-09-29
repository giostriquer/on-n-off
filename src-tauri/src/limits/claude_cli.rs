//! A Claude account's usage as Claude Code itself reports it: `claude -p /usage`, run with the
//! account's own config dir, answers from Anthropic's usage endpoint without a model turn, and
//! renews that dir's login when it has to. on-n-off sends no request and reads no credential here.
//!
//! The answer is the assistant message of `--output-format stream-json`, whose
//! `usage_report.rate_limits.limits[]` has the shape `/api/oauth/usage` answers with, so
//! [`parse_claude`] reads it. Whose answer it is comes from the config dir's `.claude.json`, which
//! Claude Code writes at sign-in and may rewrite while it runs: it must name the expected account
//! before Claude Code starts and still name it once Claude Code has answered. A config dir signed
//! out of Claude answers with no report; Claude Code's own `auth status` then tells that apart from
//! a report that could not be had.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use super::claude::parse_claude;
use super::credentials::{read_claude_config_account, ClaudeConfigAccount};
use super::{saved_card, Parsed, SavedReadError};
use crate::accounts::claude_store::ENV_CREDENTIALS;
use crate::accounts::model::Identity;
use crate::dto::{LimitWindowDto, ProviderLimitsDto, Reading};
use crate::http::HttpError;
use crate::process::{wait_with_deadline, CommandOutcome};

/// Claude Code's own usage report, printed as structured events, and no session left behind.
const USAGE_ARGS: [&str; 6] = [
    "-p",
    "/usage",
    "--no-session-persistence",
    "--output-format",
    "stream-json",
    "--verbose",
];

/// Generous, because a read that renews the login waits on Anthropic, and Claude Code killed in the
/// middle of a renewal could lose the refresh token it was just issued.
const DEADLINE: Duration = Duration::from_secs(90);

/// `auth status` only reads what is stored, and renews nothing.
const STATUS_DEADLINE: Duration = Duration::from_secs(30);

/// The usage of `identity`, reported by a `claude` from `claude` (one for the config dir whose
/// `.claude.json` is `config_file`) as its card.
pub(crate) fn read_usage(
    claude: &dyn Fn() -> Command,
    config_file: &Path,
    identity: &Identity,
) -> Result<ProviderLimitsDto, SavedReadError> {
    read_usage_within(claude, config_file, identity, DEADLINE)
}

fn read_usage_within(
    claude: &dyn Fn() -> Command,
    config_file: &Path,
    identity: &Identity,
    deadline: Duration,
) -> Result<ProviderLimitsDto, SavedReadError> {
    config_account(config_file, identity)?;
    let stdout = crate::accounts::native::run(&mut prepared(claude(), &USAGE_ARGS), deadline)
        .map_err(|_| HttpError::Network("Claude Code could not report usage.".into()))?;
    let Some(windows) = report_windows(&stdout) else {
        return Err(if signed_out(claude) {
            HttpError::Unauthorized
        } else {
            HttpError::Parse("Claude Code reported no usage.".into())
        }
        .into());
    };
    let account = config_account(config_file, identity)?;
    saved_card(
        identity,
        Parsed {
            account: Some(account.identity.account),
            reading: Reading {
                plan: account.plan,
                windows,
                ..Reading::default()
            },
        },
    )
}

/// `command` as the read runs it, with `args`.
fn prepared(mut command: Command, args: &[&str]) -> Command {
    command.args(args).env("DISABLE_AUTOUPDATER", "1");
    // Claude Code would read as any of these credentials' account, not the config dir's.
    for name in ENV_CREDENTIALS {
        command.env_remove(name);
    }
    command
}

/// Whether Claude Code says the config dir is signed out. Anything else it says, or saying nothing,
/// is no reason to ask for a new sign-in. Claude Code answers a signed-out `auth status` with exit
/// status 1, the answer on stdout all the same, so the status is read whatever the exit.
fn signed_out(claude: &dyn Fn() -> Command) -> bool {
    let mut command = prepared(claude(), &["auth", "status", "--json"]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(child) = command.spawn() else {
        return false;
    };
    let Ok(CommandOutcome::Exited { stdout, .. }) = wait_with_deadline(child, STATUS_DEADLINE)
    else {
        return false;
    };
    serde_json::from_str::<Value>(&stdout)
        .ok()
        .and_then(|status| status.get("loggedIn")?.as_bool())
        == Some(false)
}

/// The account `config_file` names, when it is `identity`'s.
fn config_account(
    config_file: &Path,
    identity: &Identity,
) -> Result<ClaudeConfigAccount, SavedReadError> {
    let account = read_claude_config_account(config_file).ok_or(HttpError::Unauthorized)?;
    if !account.identity.names(identity) {
        return Err(SavedReadError::OtherAccount);
    }
    Ok(account)
}

/// The windows of the first usage report in Claude Code's `stream-json` output.
fn report_windows(stdout: &str) -> Option<Vec<LimitWindowDto>> {
    let report = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("assistant"))
        .find_map(|event| event.pointer("/usage_report/rate_limits").cloned())?;
    Some(parse_claude(&report))
}

#[cfg(test)]
mod tests;
