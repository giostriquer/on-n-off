use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::{json, Value};

use super::codex::ResetSpend;
use super::Parsed;
use crate::accounts::codex_store::CodexAccess;
use crate::cli::AgentCli;
use crate::cli_locate::resolve_provider_cli;
use crate::dto::{AgentId, ResetCreditOutcome};
use crate::json_line_session::{Failure, NotStarted, Peer, Request, Session};

const APP_SERVER: Peer = Peer::jsonrpc_lite(
    "Codex app-server",
    "Codex CLI exited before app-server initialized. This version may not support `codex app-server`; update Codex CLI, then refresh here.",
);

#[derive(Debug)]
struct AppServerResult {
    codex_home: PathBuf,
    account: AccountReadResponse,
    rate_limits: super::codex::RateLimitsResponse,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeResponse {
    codex_home: PathBuf,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct AccountReadResponse {
    account: Option<CodexAccount>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct CodexAccount {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    plan_type: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum AppServerFailure {
    SignedOut,
    Unsupported(String),
    Failed(String),
}

#[derive(Debug, Deserialize)]
struct ConsumeResetCreditResponse {
    outcome: ResetCreditOutcome,
}

pub(super) fn consume_reset_credit(
    home: &Path,
    account_id: &str,
    idempotency_key: &str,
    how: ResetSpend,
) -> Result<ResetCreditOutcome, String> {
    spend_reset_credit(
        &home.join(".codex"),
        account_id,
        idempotency_key,
        how,
        crate::accounts::codex_store::metadata,
        start,
    )
}

fn spend_reset_credit(
    codex_home: &Path,
    account_id: &str,
    idempotency_key: &str,
    how: ResetSpend,
    identity: impl Fn(&Path) -> Result<Option<(String, Value)>, String>,
    start: impl FnOnce(&Path) -> Result<Session, String>,
) -> Result<ResetCreditOutcome, String> {
    reset_target_matches(identity(codex_home)?, account_id)?;
    start(codex_home)?.run(|session| {
        spend_in_session(
            codex_home,
            account_id,
            idempotency_key,
            how,
            &identity,
            session,
        )
    })
}

fn spend_in_session(
    codex_home: &Path,
    account_id: &str,
    idempotency_key: &str,
    how: ResetSpend,
    identity: &impl Fn(&Path) -> Result<Option<(String, Value)>, String>,
    session: &mut Session,
) -> Result<ResetCreditOutcome, Failure> {
    let (_, account) = handshake(codex_home, false, session)?;
    require_chatgpt(&account).map_err(|failure| {
        Failure::new(match failure {
            AppServerFailure::SignedOut => {
                "Codex is not signed in, so there is no banked reset to use."
            }
            _ => "Banked resets belong to a ChatGPT sign-in, and Codex is not using one.",
        })
    })?;
    identity(codex_home)
        .and_then(|current| reset_target_matches(current, account_id))
        .map_err(Failure::new)?;
    if let ResetSpend::Automatic { max_left_percent } = how {
        let rate_limits = read_rate_limits(session)?;
        spend_allowed(&super::codex::parse_codex(&rate_limits), max_left_percent)
            .map_err(Failure::new)?;
    }
    let response: ConsumeResetCreditResponse = session.request(
        Request::new(4, "account/rateLimitResetCredit/consume")
            .params(json!({"idempotencyKey": idempotency_key}))
            .when_missing(
                "Update Codex CLI to a version that supports banked resets, then try again.",
            ),
    )?;
    Ok(response.outcome)
}

fn spend_allowed(reading: &crate::dto::Reading, max_left_percent: u8) -> Result<(), String> {
    let left = reading.limit_left_percent().ok_or(
        "on-n-off can't tell how much of the Codex limit is left, so it won't spend a reset.",
    )?;
    if left <= f64::from(max_left_percent) {
        return Ok(());
    }
    Err(format!(
        "A banked reset can be used once {max_left_percent}% or less of the current limit is left, and {}% is left.",
        left.round()
    ))
}

fn reset_target_matches(current: Option<(String, Value)>, account_id: &str) -> Result<(), String> {
    match current {
        Some((id, _)) if id == account_id => Ok(()),
        Some(_) => Err("The signed-in Codex account changed. Try again on its card.".to_string()),
        None => Err("on-n-off can't confirm which Codex account is signed in, so it won't spend a reset. Sign in again with `codex`.".to_string()),
    }
}

pub(super) fn read(
    home: &Path,
    force: bool,
    after_identity: impl FnOnce(&mut Parsed, Option<&CodexAccess>),
) -> Result<Parsed, AppServerFailure> {
    read_with(home, force, start, after_identity)
}

fn read_with(
    home: &Path,
    force: bool,
    start: impl FnOnce(&Path) -> Result<Session, String>,
    after_identity: impl FnOnce(&mut Parsed, Option<&CodexAccess>),
) -> Result<Parsed, AppServerFailure> {
    let expected_codex_home = home.join(".codex");
    let before = crate::accounts::codex_store::metadata(&expected_codex_home)
        .map_err(AppServerFailure::Failed)?;
    let session = start(&expected_codex_home)
        .and_then(|session| {
            session.run(|session| query_app_server(&expected_codex_home, force, session))
        })
        .map_err(AppServerFailure::Failed)?;
    let (mut parsed, access) = normalize_app_server(session, before)?;
    after_identity(&mut parsed, access.as_ref());
    Ok(parsed)
}

fn start(codex_home: &Path) -> Result<Session, String> {
    let binary = resolve_provider_cli(AgentId::Codex, "codex")
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "codex".to_string());
    let mut command: Command = AgentCli::new(binary).command();
    command
        .args(["app-server", "--stdio"])
        .env("CODEX_HOME", codex_home);
    begin(APP_SERVER, command)
}

fn begin(peer: Peer, command: Command) -> Result<Session, String> {
    Session::start(peer, command).map_err(|failure| match failure {
        NotStarted::Missing => {
            "Codex CLI not found. Install or configure `codex`, then refresh Limits.".to_string()
        }
        NotStarted::Failed(message) => message,
    })
}

fn query_app_server(
    expected_codex_home: &Path,
    force: bool,
    session: &mut Session,
) -> Result<AppServerResult, Failure> {
    let (codex_home, account) = handshake(expected_codex_home, force, session)?;
    let rate_limits = read_rate_limits(session)?;
    Ok(AppServerResult {
        codex_home,
        account,
        rate_limits,
    })
}

fn read_rate_limits(session: &mut Session) -> Result<super::codex::RateLimitsResponse, Failure> {
    session.request(Request::new(3, "account/rateLimits/read").when_missing(
        "Update Codex CLI to a version that supports subscription limits, then refresh here.",
    ))
}

fn handshake(
    expected_codex_home: &Path,
    force: bool,
    session: &mut Session,
) -> Result<(PathBuf, AccountReadResponse), Failure> {
    let initialize: InitializeResponse =
        session.handshake(None, |initialize: &InitializeResponse| {
            if crate::paths::equivalent(&initialize.codex_home, expected_codex_home) {
                return Ok(());
            }
            Err(format!(
                "Codex app-server used {}, expected {}.",
                initialize.codex_home.display(),
                expected_codex_home.display()
            ))
        })?;
    let account =
        session.request(Request::new(2, "account/read").params(json!({"refreshToken": force})))?;
    Ok((initialize.codex_home, account))
}

fn normalize_app_server(
    session: AppServerResult,
    before: Option<(String, Value)>,
) -> Result<(Parsed, Option<CodexAccess>), AppServerFailure> {
    let account = require_chatgpt(&session.account)?;
    let email = account
        .email
        .as_deref()
        .map(str::trim)
        .filter(|email| !email.is_empty())
        .map(str::to_string);
    let mut reading = super::codex::parse_codex(&session.rate_limits);
    reading.plan = account
        .plan_type
        .as_deref()
        .map(str::trim)
        .filter(|plan| !plan.is_empty())
        .map(str::to_string)
        .or(reading.plan);
    let (after, access) = crate::accounts::codex_store::metadata_and_access(&session.codex_home)
        .map_err(AppServerFailure::Failed)?
        .map_or((None, None), |(metadata, access)| (Some(metadata), access));
    if before.as_ref().map(|(id, _)| id) != after.as_ref().map(|(id, _)| id) {
        return Err(AppServerFailure::Failed(
            "The signed-in account changed while reading usage. Retry after sign-in finishes."
                .into(),
        ));
    }
    let metadata = before;
    let legacy_id = metadata.as_ref().and_then(|(id, claims)| {
        let workspace = claims.get("chatgpt_account_id")?.as_str()?;
        (id != workspace).then(|| workspace.to_owned())
    });
    let account_id = metadata
        .map(|(id, _)| id)
        .or_else(|| {
            email
                .as_deref()
                .map(|email| format!("email:{}", email.to_lowercase()))
        })
        .unwrap_or_else(|| super::DEFAULT_ACCOUNT.to_string());
    let parsed = Parsed {
        account: Some(crate::dto::LimitsAccountDto {
            legacy_id,
            id: account_id,
            label: email,
        }),
        reading,
    };
    Ok((parsed, access))
}

fn require_chatgpt(read: &AccountReadResponse) -> Result<&CodexAccount, AppServerFailure> {
    let Some(account) = read.account.as_ref() else {
        return Err(AppServerFailure::SignedOut);
    };
    match account.kind.as_str() {
        "chatgpt" => Ok(account),
        "apiKey" => Err(AppServerFailure::Unsupported(
            "Codex is signed in with an API key, which has no subscription limits.".to_string(),
        )),
        other => Err(AppServerFailure::Unsupported(format!(
            "Codex account type `{other}` has no subscription limits to show."
        ))),
    }
}

#[cfg(test)]
mod tests;
