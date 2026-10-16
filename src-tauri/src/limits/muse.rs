// Muse subscription limits, read the way Muse exposes them: `muse serve`
// speaks MSP over stdio, and its experimental `usage/read` reports the last
// subscription usage the host observed from a provider frame. on-n-off opens
// no Muse credential and never starts a model turn to poll: when the fresh
// host has observed nothing, the card says so and keeps its account so the
// remembered reading stands in.
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{DateTime, SecondsFormat};
use serde::Deserialize;
use serde_json::json;

use super::pipeline::finish;
use super::Parsed;
use crate::cli::AgentCli;
use crate::cli_locate::resolve_provider_cli;
use crate::dto::{
    AgentId, LimitWindowDto, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto,
    Reading,
};
use crate::json_line_session::{Conversation, Failure, NotStarted, Peer, Request, Session};

const SERVE: Peer = Peer::jsonrpc(
    "Muse serve",
    "Muse CLI exited before `muse serve` initialized. This version may not support `muse serve`; update Muse Code, then refresh here.",
);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeResult {
    muse_home: PathBuf,
    #[serde(default)]
    experimental_api: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountState {
    state: String,
    #[serde(default)]
    label: Option<String>,
    credential_required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscriptionUsage {
    tier: String,
    window: UsageWindow,
    weekly: UsageWeekly,
    observed_at_ms: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageWindow {
    used_percent: f64,
    window_duration_mins: u64,
    resets_at_ms: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageWeekly {
    used_percent: f64,
    resets_at_ms: i64,
}

#[derive(Debug)]
struct ServeAnswers {
    account: AccountState,
    usage: Option<SubscriptionUsage>,
}

#[derive(Debug, PartialEq, Eq)]
enum ServeFailure {
    NotInstalled,
    SignedOut,
    Unsupported(String),
    NoUsageYet(LimitsAccountDto),
    Failed(String),
}

fn query_serve(session: Session, expected_data: &Path) -> Result<ServeAnswers, String> {
    session.run(
        Some(json!({"experimentalApi": true, "userInputDialogs": false})),
        |initialize: &InitializeResult| {
            if !crate::paths::equivalent(&initialize.muse_home, expected_data) {
                return Err(format!(
                    "Muse serve used {}, expected {}.",
                    initialize.muse_home.display(),
                    expected_data.display()
                ));
            }
            if !initialize.experimental_api {
                return Err(
                    "Update Muse: this version cannot report subscription usage to on-n-off."
                        .to_string(),
                );
            }
            Ok(())
        },
        |_, conversation| read_account_usage(conversation),
    )
}

fn read_account_usage(conversation: &mut Conversation) -> Result<ServeAnswers, Failure> {
    let account: AccountState = conversation.request(Request::new(2, "account/read"))?;
    if account.state != "accountLogin" {
        return Ok(ServeAnswers {
            account,
            usage: None,
        });
    }
    let usage_read: UsageReadResult =
        conversation.request(Request::new(3, "usage/read").when_missing(
            "Update Muse to a version that reports subscription usage, then refresh here.",
        ))?;
    let after: AccountState = conversation.request(Request::new(4, "account/read"))?;
    if after != account {
        return Err(Failure::new(
            "The signed-in Muse account changed while its usage was read.",
        ));
    }
    Ok(ServeAnswers {
        account,
        usage: usage_read.usage,
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageReadResult {
    #[serde(default)]
    usage: Option<SubscriptionUsage>,
}

fn normalize(answers: ServeAnswers) -> Result<Parsed, ServeFailure> {
    let account = &answers.account;
    match account.state.as_str() {
        "accountLogin" => {}
        "loggedOut" => return Err(ServeFailure::SignedOut),
        "apiKey" | "envKey" => {
            return Err(ServeFailure::Unsupported(
                "Muse is signed in with an API key, which has no subscription limits.".to_string(),
            ));
        }
        other => {
            return Err(ServeFailure::Unsupported(format!(
                "Muse account type `{other}` has no subscription limits to show."
            )));
        }
    }
    if !account.credential_required {
        return Err(ServeFailure::Unsupported(
            "This Muse endpoint needs no credential and reports no subscription limits."
                .to_string(),
        ));
    }
    let label = account
        .label
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_string);
    let account_dto = LimitsAccountDto {
        legacy_id: None,
        id: label
            .as_deref()
            .filter(|label| label.contains('@'))
            .map(|label| format!("email:{}", label.to_lowercase()))
            .unwrap_or_else(|| super::DEFAULT_ACCOUNT.to_string()),
        label: label.clone(),
    };
    let Some(usage) = answers.usage else {
        return Err(ServeFailure::NoUsageYet(account_dto));
    };
    let observed_at = rfc3339_ms(usage.observed_at_ms);
    let mut session_window = window(
        "session",
        session_label(usage.window.window_duration_mins),
        LimitWindowKind::Session,
        usage.window.used_percent,
        usage.window.resets_at_ms,
        &observed_at,
    );
    session_window.window_seconds = Some(usage.window.window_duration_mins.saturating_mul(60));
    let weekly_window = window(
        "weekly",
        "Weekly · all models",
        LimitWindowKind::Weekly,
        usage.weekly.used_percent,
        usage.weekly.resets_at_ms,
        &observed_at,
    );
    Ok(Parsed {
        account: Some(account_dto),
        reading: Reading {
            // The tier arrives as an opaque numeric id, not a plan name;
            // keep it only when it reads as one.
            plan: Some(usage.tier.trim().to_string())
                .filter(|tier| tier.chars().any(|c| c.is_alphabetic())),
            windows: vec![weekly_window, session_window],
            ..Reading::default()
        },
    })
}

fn session_label(minutes: u64) -> String {
    if minutes > 0 && minutes.is_multiple_of(60) {
        format!("{} hour · all models", minutes / 60)
    } else {
        format!("{minutes} min · all models")
    }
}

fn window(
    id: &str,
    label: impl Into<String>,
    kind: LimitWindowKind,
    used_percent: f64,
    resets_at_ms: i64,
    observed_at: &str,
) -> LimitWindowDto {
    LimitWindowDto {
        id: id.to_string(),
        label: label.into(),
        kind,
        used_percent: if used_percent.is_finite() {
            used_percent.clamp(0.0, 100.0)
        } else {
            0.0
        },
        resets_at: (resets_at_ms > 0).then(|| rfc3339_ms(resets_at_ms)),
        window_seconds: None,
        observed_at: observed_at.to_string(),
    }
}

fn rfc3339_ms(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms)
        .map(|at| at.to_rfc3339_opts(SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn finish_read(read: Result<Parsed, ServeFailure>) -> ProviderLimitsDto {
    match read {
        Ok(parsed) => finish(AgentId::Muse, LimitsStatus::Ok, None, parsed),
        Err(ServeFailure::NotInstalled) => finish(
            AgentId::Muse,
            LimitsStatus::SignedOut,
            Some(
                "Install Muse Code and sign in with `muse` to see subscription limits.".to_string(),
            ),
            Parsed::default(),
        ),
        Err(ServeFailure::SignedOut) => finish(
            AgentId::Muse,
            LimitsStatus::SignedOut,
            Some("Sign in with `muse` to see subscription limits.".to_string()),
            Parsed::default(),
        ),
        Err(ServeFailure::Unsupported(message)) => finish(
            AgentId::Muse,
            LimitsStatus::Unsupported,
            Some(message),
            Parsed::default(),
        ),
        Err(ServeFailure::NoUsageYet(account)) => finish(
            AgentId::Muse,
            LimitsStatus::Failed,
            Some("Muse has not observed subscription usage yet. This version does not expose usage to external readers.".to_string()),
            Parsed {
                account: Some(account),
                ..Parsed::default()
            },
        ),
        Err(ServeFailure::Failed(message)) => finish(
            AgentId::Muse,
            LimitsStatus::Failed,
            Some(message),
            Parsed::default(),
        ),
    }
}

pub(super) fn muse_current(home: &Path) -> ProviderLimitsDto {
    finish_read(read_with(home, start))
}

fn read_with(
    home: &Path,
    start: impl FnOnce(&Path, &Path, &Path) -> Result<Session, ServeFailure>,
) -> Result<Parsed, ServeFailure> {
    let config = crate::paths::muse_config_for(home);
    let data = crate::paths::muse_data_for(home);
    let answers = query_serve(start(home, &config, &data)?, &data).map_err(ServeFailure::Failed)?;
    normalize(answers)
}

fn start(home: &Path, config: &Path, data: &Path) -> Result<Session, ServeFailure> {
    let binary = resolve_provider_cli(AgentId::Muse, "muse")
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "muse".to_string());
    let mut command: Command = AgentCli::new(binary).command();
    command
        .args([
            "serve",
            "--no-session-log",
            "--disable-shell",
            "--disable-write",
        ])
        .env("HOME", home);
    if let Some(parent) = config.parent() {
        command.env("XDG_CONFIG_HOME", parent);
    }
    if let Some(parent) = data.parent() {
        command.env("XDG_DATA_HOME", parent);
    }
    #[cfg(windows)]
    command.env("USERPROFILE", home);
    begin(SERVE, command)
}

fn begin(peer: Peer, command: Command) -> Result<Session, ServeFailure> {
    Session::start(peer, command).map_err(|failure| match failure {
        NotStarted::Missing => ServeFailure::NotInstalled,
        NotStarted::Failed(message) => ServeFailure::Failed(message),
    })
}

#[cfg(test)]
mod tests;
