//! A Claude account's usage as Claude Code itself reports it: `claude -p /usage`, run with the
//! account's own config dir, answers from Anthropic's usage endpoint without a model turn, and
//! renews that dir's login when it has to. on-n-off sends no request and reads no credential here.
//!
//! The answer is the assistant message of `--output-format stream-json`, whose
//! `usage_report.rate_limits.limits[]` has the shape `/api/oauth/usage` answers with, so
//! [`parse_claude`] reads it. Whose answer it is comes from the config dir's `.claude.json`, which
//! Claude Code writes at sign-in and may rewrite while it runs: it must name the expected account
//! before Claude Code starts and still name it once Claude Code has answered.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use super::claude::parse_claude;
use super::credentials::{claude_identity, plan_label, read_claude_account};
use super::json::optional_string;
use super::{saved_card, Parsed, SavedReadError};
use crate::accounts::model::Identity;
use crate::dto::{LimitWindowDto, LimitsAccountDto, ProviderLimitsDto, Reading};
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

/// Credentials Claude Code prefers to the config dir's login, which would make the report another
/// account's.
const ENV_CREDENTIALS: [&str; 3] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
];

/// The usage of `identity`, reported by `command` (a `claude` for the config dir whose
/// `.claude.json` is `config_file`) as its card.
pub(crate) fn read_usage(
    command: Command,
    config_file: &Path,
    identity: &Identity,
) -> Result<ProviderLimitsDto, SavedReadError> {
    read_usage_within(command, config_file, identity, DEADLINE)
}

fn read_usage_within(
    command: Command,
    config_file: &Path,
    identity: &Identity,
    deadline: Duration,
) -> Result<ProviderLimitsDto, SavedReadError> {
    config_account(config_file, identity)?;
    let child = prepared(command)
        .spawn()
        .map_err(|error| HttpError::Network(format!("Cannot start Claude Code: {error}")))?;
    let stdout = match wait_with_deadline(child, deadline) {
        Ok(CommandOutcome::Exited {
            success: true,
            stdout,
            ..
        }) => stdout,
        Ok(CommandOutcome::Exited { .. }) => {
            return Err(HttpError::Network("Claude Code could not report usage.".into()).into())
        }
        Ok(CommandOutcome::TimedOut) => {
            return Err(
                HttpError::Network("Claude Code did not report usage in time.".into()).into(),
            )
        }
        Err(error) => return Err(HttpError::Network(error.to_string()).into()),
    };
    let windows = report_windows(&stdout)
        .ok_or_else(|| HttpError::Parse("Claude Code reported no usage.".into()))?;
    let (account, plan) = config_account(config_file, identity)?;
    saved_card(
        identity,
        Parsed {
            account: Some(account),
            reading: Reading {
                plan,
                windows,
                ..Reading::default()
            },
        },
    )
}

/// `command` as the usage read runs it.
fn prepared(mut command: Command) -> Command {
    command
        .args(USAGE_ARGS)
        .env("DISABLE_AUTOUPDATER", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in ENV_CREDENTIALS {
        command.env_remove(name);
    }
    command
}

/// The account `config_file` names, when it is `identity`'s, with the plan its organization is on.
fn config_account(
    config_file: &Path,
    identity: &Identity,
) -> Result<(LimitsAccountDto, Option<String>), SavedReadError> {
    let record = read_claude_account(config_file).ok_or(HttpError::Unauthorized)?;
    let named = claude_identity(&record).ok_or(HttpError::Unauthorized)?;
    if named.account.id != identity.user_id
        || named.organization_id.as_deref() != Some(identity.workspace_id.as_str())
    {
        return Err(SavedReadError::OtherAccount);
    }
    // `organizationType` names the subscription as `claude_<type>`; `<type>` is what the stored
    // login's `subscriptionType` says.
    let organization_type = optional_string(record.get("organizationType"));
    let subscription = organization_type
        .as_deref()
        .and_then(|kind| kind.strip_prefix("claude_"));
    let tier = optional_string(record.get("organizationRateLimitTier"));
    Ok((named.account, plan_label(subscription, tier.as_deref())))
}

/// The windows of the first usage report in Claude Code's `stream-json` output, when it names any
/// this reader knows.
fn report_windows(stdout: &str) -> Option<Vec<LimitWindowDto>> {
    let report = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|event| {
            (event.get("type").and_then(Value::as_str) == Some("assistant"))
                .then(|| event.pointer("/usage_report/rate_limits").cloned())
                .flatten()
        })?;
    Some(parse_claude(&report)).filter(|windows| !windows.is_empty())
}

#[cfg(test)]
mod tests;
