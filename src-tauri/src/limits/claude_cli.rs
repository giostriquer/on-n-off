use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use super::claude::parse_claude;
use super::claude_config::{read_claude_config_account, ClaudeConfigAccount};
use super::outcome::{saved_card, Accepted, Read, ReadFailure};
use super::{scoped_account, Parsed, DEFAULT_ACCOUNT};
use crate::accounts::claude_store::ENV_CREDENTIALS;
use crate::accounts::model::Identity;
use crate::dto::{AgentId, LimitWindowDto, LimitsAccountDto, Reading};
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

const UNREPORTED: &str = "Claude Code could not report usage.";

const DEADLINE: Duration = Duration::from_secs(90);

const STATUS_DEADLINE: Duration = Duration::from_secs(30);

pub(crate) fn read_usage(
    claude: &dyn Fn() -> Command,
    config_file: &Path,
    identity: &Identity,
) -> Result<Accepted, ReadFailure> {
    read_usage_within(claude, config_file, identity, DEADLINE)
}

fn read_usage_within(
    claude: &dyn Fn() -> Command,
    config_file: &Path,
    identity: &Identity,
    deadline: Duration,
) -> Result<Accepted, ReadFailure> {
    config_account(config_file, identity)?;
    let windows = report(claude, deadline)?;
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

pub(super) fn read_signed_in(
    claude: &dyn Fn(&Path) -> Command,
    config_file: &Path,
    usage_config_dir: &Path,
) -> Read {
    read_signed_in_within(claude, config_file, usage_config_dir, DEADLINE)
}

fn read_signed_in_within(
    claude: &dyn Fn(&Path) -> Command,
    config_file: &Path,
    usage_config_dir: &Path,
    deadline: Duration,
) -> Read {
    let before = read_claude_config_account(config_file);
    let answer = (|| {
        clear_if_another_account(usage_config_dir, before.as_ref())
            .map_err(ReadFailure::Unavailable)?;
        let windows = report(&|| claude(usage_config_dir), deadline)?;
        let after = read_claude_config_account(config_file);
        if !same_account(before.as_ref(), after.as_ref()) {
            let _ = std::fs::remove_dir_all(usage_config_dir);
            return Err(ReadFailure::AccountChanged);
        }
        Ok(Parsed {
            account: Some(account_named(after.as_ref())),
            reading: Reading {
                plan: after.and_then(|account| account.plan),
                windows,
                ..Reading::default()
            },
        })
    })();
    Read {
        account: Some(account_named(before.as_ref())),
        answer,
    }
}

pub(super) fn known_account(config_file: &Path) -> LimitsAccountDto {
    account_named(read_claude_config_account(config_file).as_ref())
}

fn account_named(account: Option<&ClaudeConfigAccount>) -> LimitsAccountDto {
    account.map_or_else(default_account, card_account)
}

fn same_account(a: Option<&ClaudeConfigAccount>, b: Option<&ClaudeConfigAccount>) -> bool {
    a.map(|a| a.identity.account_key()) == b.map(|b| b.identity.account_key())
}

fn clear_if_another_account(
    usage_config_dir: &Path,
    account: Option<&ClaudeConfigAccount>,
) -> Result<(), String> {
    let recorded = read_claude_config_account(&usage_config_dir.join(".claude.json"));
    if same_account(recorded.as_ref(), account) {
        return Ok(());
    }
    match std::fs::remove_dir_all(usage_config_dir) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(format!(
            "Could not clear Claude Code's usage config dir ({}): {error}",
            usage_config_dir.display()
        )),
        _ => Ok(()),
    }
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

fn report(
    claude: &dyn Fn() -> Command,
    deadline: Duration,
) -> Result<Vec<LimitWindowDto>, ReadFailure> {
    let unreported = || ReadFailure::Unavailable(UNREPORTED.to_string());
    let mut command = prepared(claude(), &USAGE_ARGS);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => ReadFailure::NotInstalled,
        _ => unreported(),
    })?;
    match wait_with_deadline(child, deadline) {
        Ok(CommandOutcome::Exited {
            success: true,
            stdout,
            ..
        }) => {
            let windows = report_windows(&stdout).unwrap_or_default();
            if windows.is_empty() && signed_out(claude) {
                return Err(ReadFailure::SignedOut);
            }
            Ok(windows)
        }
        Ok(CommandOutcome::Exited { stderr, .. })
            if stderr.contains("unknown option") && stderr.contains("--safe-mode") =>
        {
            Err(ReadFailure::Unavailable(OUTDATED.to_string()))
        }
        _ => Err(unreported()),
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
) -> Result<ClaudeConfigAccount, ReadFailure> {
    let account = read_claude_config_account(config_file).ok_or(ReadFailure::SignedOut)?;
    if !account.identity.names(identity) {
        return Err(ReadFailure::OtherAccount);
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
