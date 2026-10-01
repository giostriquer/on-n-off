use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use super::claude::parse_claude;
use super::claude_config::{read_claude_config_account, ClaudeConfigAccount};
use super::pipeline::finish;
use super::{saved_card, scoped_account, Parsed, SavedReadError, DEFAULT_ACCOUNT};
use crate::accounts::claude_store::ENV_CREDENTIALS;
use crate::accounts::model::Identity;
use crate::dto::{
    AgentId, LimitWindowDto, LimitsAccountDto, LimitsStatus, ProviderLimitsDto, Reading,
};
use crate::http::HttpError;
use crate::process::{wait_with_deadline, CommandOutcome};

const USAGE_ARGS: [&str; 7] = [
    "-p",
    "/usage",
    "--no-session-persistence",
    "--safe-mode",
    "--output-format",
    "stream-json",
    "--verbose",
];

pub(crate) const OUTDATED: &str =
    "Update Claude Code: this version cannot report usage without starting your hooks and MCP servers.";

const DEADLINE: Duration = Duration::from_secs(90);

const STATUS_DEADLINE: Duration = Duration::from_secs(30);

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
        NoReport::SignedOut => SavedReadError::Http(HttpError::Unauthorized),
        _ => SavedReadError::Unavailable(why.message()),
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

pub(crate) fn read_signed_in(
    claude: &dyn Fn() -> Command,
    without_history: Option<&dyn Fn() -> Command>,
    config_file: &Path,
) -> ProviderLimitsDto {
    read_signed_in_within(claude, without_history, config_file, DEADLINE)
}

fn read_signed_in_within(
    claude: &dyn Fn() -> Command,
    without_history: Option<&dyn Fn() -> Command>,
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
    let read = match without_history.map(|claude| report(claude, deadline)) {
        None | Some(Err(NoReport::SignedOut)) => report(claude, deadline),
        Some(read) => read,
    };
    let windows = match read {
        Ok(windows) => windows,
        Err(NoReport::SignedOut) => {
            return failed(
                LimitsStatus::SignedOut,
                "Sign in with `claude` to see subscription limits.",
            )
        }
        Err(NoReport::NotInstalled) => {
            return failed(
                LimitsStatus::SignedOut,
                "Install Claude Code and sign in with `claude` to see subscription limits.",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoReport {
    NotInstalled,
    Unavailable,
    Outdated,
    SignedOut,
    Empty,
}

impl NoReport {
    fn message(self) -> &'static str {
        match self {
            Self::NotInstalled => "Claude Code is not installed.",
            Self::Unavailable => "Claude Code could not report usage.",
            Self::Outdated => OUTDATED,
            Self::SignedOut => "Claude Code is signed out.",
            Self::Empty => "Claude Code reported no usage.",
        }
    }
}

fn report(
    claude: &dyn Fn() -> Command,
    deadline: Duration,
) -> Result<Vec<LimitWindowDto>, NoReport> {
    let mut command = prepared(claude(), &USAGE_ARGS);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => NoReport::NotInstalled,
        _ => NoReport::Unavailable,
    })?;
    match wait_with_deadline(child, deadline) {
        Ok(CommandOutcome::Exited {
            success: true,
            stdout,
            ..
        }) => report_windows(&stdout)
            .filter(|windows| !windows.is_empty())
            .ok_or_else(|| {
                if signed_out(claude) {
                    NoReport::SignedOut
                } else {
                    NoReport::Empty
                }
            }),
        Ok(CommandOutcome::Exited { stderr, .. })
            if stderr.contains("unknown option") && stderr.contains("--safe-mode") =>
        {
            Err(NoReport::Outdated)
        }
        _ => Err(NoReport::Unavailable),
    }
}

fn prepared(mut command: Command, args: &[&str]) -> Command {
    command.args(args).env("DISABLE_AUTOUPDATER", "1");
    for name in ENV_CREDENTIALS {
        command.env_remove(name);
    }
    command
}

fn signed_out(claude: &dyn Fn() -> Command) -> bool {
    auth_status(claude).and_then(|status| status.get("loggedIn")?.as_bool()) == Some(false)
}

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
