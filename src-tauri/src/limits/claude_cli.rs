//! A Claude account's usage as Claude Code itself reports it: `claude -p /usage`, run with the
//! account's own config dir, answers from Anthropic's usage endpoint without a model turn, and
//! renews that dir's login when it has to. on-n-off sends no request and reads no credential here.
//! It reads both the signed-in account, in the user's own config dir ([`read_signed_in`]), and a
//! saved account in its home ([`read_usage`]).
//!
//! The user's own config dir has their hooks, plugins, MCP servers and CLAUDE.md, which every poll
//! would otherwise start, so each read runs with `--safe-mode`, which leaves them all out and keeps
//! the login. A Claude Code too old to know the flag is not asked at all.
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
use super::pipeline::finish;
use super::{saved_card, scoped_account, Parsed, SavedReadError, DEFAULT_ACCOUNT};
use crate::accounts::claude_store::ENV_CREDENTIALS;
use crate::accounts::model::Identity;
use crate::dto::{
    AgentId, LimitWindowDto, LimitsAccountDto, LimitsStatus, ProviderLimitsDto, Reading,
};
use crate::http::HttpError;
use crate::process::{wait_with_deadline, CommandOutcome};

/// Claude Code's own usage report, printed as structured events, with no session left behind and
/// none of the config dir's customizations started.
const USAGE_ARGS: [&str; 7] = [
    "-p",
    "/usage",
    "--no-session-persistence",
    "--safe-mode",
    "--output-format",
    "stream-json",
    "--verbose",
];

/// What a Claude Code that does not know `--safe-mode` is told to do.
pub(crate) const OUTDATED: &str =
    "Update Claude Code: this version cannot report usage without starting your hooks and MCP servers.";

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
    let windows = report(claude, deadline).map_err(|why| match why {
        NoReport::SignedOut => HttpError::Unauthorized,
        NoReport::Empty => HttpError::Parse(why.message().into()),
        NoReport::Unavailable | NoReport::Outdated => HttpError::Network(why.message().into()),
    })?;
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

/// The signed-in account's card, as a `claude` from `claude` reports it for the user's own config
/// dir, whose `.claude.json` is `config_file`. The card is the account that file names before the
/// read, which must still name it after; a read that fails keeps that account, so the card it
/// remembers stands in.
pub(crate) fn read_signed_in(
    claude: &dyn Fn() -> Command,
    config_file: &Path,
) -> ProviderLimitsDto {
    read_signed_in_within(claude, config_file, DEADLINE)
}

fn read_signed_in_within(
    claude: &dyn Fn() -> Command,
    config_file: &Path,
    deadline: Duration,
) -> ProviderLimitsDto {
    let before = read_claude_config_account(config_file);
    let account = before.as_ref().map_or_else(default_account, card_account);
    let failed = |status, message: &str| {
        finish(
            AgentId::Claude,
            status,
            Some(message.into()),
            Parsed {
                account: Some(account.clone()),
                reading: Reading::default(),
            },
        )
    };
    let windows = match report(claude, deadline) {
        Ok(windows) => windows,
        Err(NoReport::SignedOut) => {
            return failed(
                LimitsStatus::SignedOut,
                "Sign in with `claude` to see subscription limits.",
            )
        }
        Err(why) => return failed(LimitsStatus::Failed, why.message()),
    };
    let after = read_claude_config_account(config_file);
    let same = |a: &ClaudeConfigAccount, b: &ClaudeConfigAccount| {
        a.identity.account.id == b.identity.account.id
            && a.identity.organization_id == b.identity.organization_id
    };
    match (&before, &after) {
        (Some(before), Some(after)) if same(before, after) => {}
        (None, None) => {}
        _ => {
            return failed(
                LimitsStatus::Failed,
                "The signed-in Claude account changed while its usage was read.",
            )
        }
    }
    finish(
        AgentId::Claude,
        LimitsStatus::Ok,
        None,
        Parsed {
            account: Some(after.as_ref().map_or_else(default_account, card_account)),
            reading: Reading {
                plan: after.and_then(|account| account.plan),
                windows,
                ..Reading::default()
            },
        },
    )
}

/// The account a signed-in card is known by: its scoped key when the config names its
/// organization, as its saved profile's card is, else the account's own id.
fn card_account(account: &ClaudeConfigAccount) -> LimitsAccountDto {
    let identity = &account.identity;
    match &identity.organization_id {
        Some(workspace) => scoped_account(
            &Identity {
                provider: AgentId::Claude,
                user_id: identity.account.id.clone(),
                workspace_id: workspace.clone(),
            },
            identity.account.label.clone(),
        ),
        None => identity.account.clone(),
    }
}

fn default_account() -> LimitsAccountDto {
    LimitsAccountDto {
        legacy_id: None,
        id: DEFAULT_ACCOUNT.to_string(),
        label: None,
    }
}

/// Why Claude Code gave no usage report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoReport {
    /// It could not be started, failed, or did not answer in time.
    Unavailable,
    /// It does not know `--safe-mode`, so it was not left to run with the user's customizations.
    Outdated,
    /// It says the config dir is signed out of Claude.
    SignedOut,
    /// It answered, with no report in the answer.
    Empty,
}

impl NoReport {
    fn message(self) -> &'static str {
        match self {
            Self::Unavailable => "Claude Code could not report usage.",
            Self::Outdated => OUTDATED,
            Self::SignedOut => "Claude Code is signed out.",
            Self::Empty => "Claude Code reported no usage.",
        }
    }
}

/// The windows of the usage report a `claude` from `claude` prints within `deadline`.
fn report(
    claude: &dyn Fn() -> Command,
    deadline: Duration,
) -> Result<Vec<LimitWindowDto>, NoReport> {
    let mut command = prepared(claude(), &USAGE_ARGS);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().map_err(|_| NoReport::Unavailable)?;
    match wait_with_deadline(child, deadline) {
        Ok(CommandOutcome::Exited {
            success: true,
            stdout,
            ..
        }) => report_windows(&stdout).ok_or_else(|| {
            if signed_out(claude) {
                NoReport::SignedOut
            } else {
                NoReport::Empty
            }
        }),
        // Claude Code without the flag says `error: unknown option '--safe-mode'`.
        Ok(CommandOutcome::Exited { stderr, .. })
            if stderr.contains("unknown option") && stderr.contains("--safe-mode") =>
        {
            Err(NoReport::Outdated)
        }
        _ => Err(NoReport::Unavailable),
    }
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
/// is no reason to ask for a new sign-in.
fn signed_out(claude: &dyn Fn() -> Command) -> bool {
    auth_status(claude).and_then(|status| status.get("loggedIn")?.as_bool()) == Some(false)
}

/// What `claude auth status --json` says of the config dir a `claude` from `claude` works in:
/// `loggedIn`, and when it is, `email`, `orgId` and the like. It reads only what is stored and asks
/// Anthropic nothing. Claude Code answers a signed-out status with exit status 1, the answer on
/// stdout all the same, so the status is read whatever the exit. `None` when it says nothing
/// readable.
pub(crate) fn auth_status(claude: &dyn Fn() -> Command) -> Option<Value> {
    let mut command = prepared(claude(), &["auth", "status", "--json"]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().ok()?;
    let Ok(CommandOutcome::Exited { stdout, .. }) = wait_with_deadline(child, STATUS_DEADLINE)
    else {
        return None;
    };
    serde_json::from_str(&stdout).ok()
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
