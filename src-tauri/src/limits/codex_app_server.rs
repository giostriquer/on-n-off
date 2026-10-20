use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::{json, Value};

use super::outcome::{Read, ReadFailure, Unsupported};
use super::Parsed;
use crate::accounts::codex_store::{CodexAccess, Metadata};
use crate::cli::AgentCli;
use crate::cli_locate::resolve_provider_cli;
use crate::dto::{AgentId, ResetCreditOutcome};
use crate::json_line_session::{Conversation, Failure, NotStarted, Peer, Request, Session};
use crate::reset_alert::ResetSpend;

const APP_SERVER: Peer = Peer::jsonrpc_lite(
    "Codex app-server",
    "Codex CLI exited before app-server initialized. This version may not support `codex app-server`; update Codex CLI, then refresh here.",
);

#[derive(Debug)]
struct AppServerAnswers {
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
    open_account(
        start(codex_home)?,
        codex_home,
        false,
        |_, account, conversation| {
            spend_in_session(
                codex_home,
                account_id,
                idempotency_key,
                how,
                &identity,
                &account,
                conversation,
            )
        },
    )
}

fn spend_in_session(
    codex_home: &Path,
    account_id: &str,
    idempotency_key: &str,
    how: ResetSpend,
    identity: &impl Fn(&Path) -> Result<Option<(String, Value)>, String>,
    account: &AccountReadResponse,
    conversation: &mut Conversation,
) -> Result<ResetCreditOutcome, Failure> {
    require_chatgpt(account).map_err(|failure| {
        Failure::new(match failure {
            ReadFailure::SignedOut => "Codex is not signed in, so there is no banked reset to use.",
            _ => "Banked resets belong to a ChatGPT sign-in, and Codex is not using one.",
        })
    })?;
    identity(codex_home)
        .and_then(|current| reset_target_matches(current, account_id))
        .map_err(Failure::new)?;
    if let ResetSpend::Automatic(share) = how {
        let rate_limits = read_rate_limits(conversation)?;
        share
            .allows(&super::codex::parse_codex(&rate_limits))
            .map_err(Failure::new)?;
    }
    let response: ConsumeResetCreditResponse = conversation.request(
        Request::new(4, "account/rateLimitResetCredit/consume")
            .params(json!({"idempotencyKey": idempotency_key}))
            .when_missing(
                "Update Codex CLI to a version that supports banked resets, then try again.",
            ),
    )?;
    Ok(response.outcome)
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
) -> Read {
    read_with(home, force, start_read, after_identity)
}

fn read_with(
    home: &Path,
    force: bool,
    start: impl FnOnce(&Path) -> Result<Session, ReadFailure>,
    after_identity: impl FnOnce(&mut Parsed, Option<&CodexAccess>),
) -> Read {
    let expected_codex_home = home.join(".codex");
    let before = match crate::accounts::codex_store::metadata(&expected_codex_home) {
        Ok(before) => before,
        Err(why) => return Read::anonymous(ReadFailure::Unavailable(why)),
    };
    let account = card_account(before.as_ref(), None);
    let answer = start(&expected_codex_home)
        .and_then(|session| {
            query_app_server(session, &expected_codex_home, force).map_err(ReadFailure::Unavailable)
        })
        .and_then(|answers| normalize_app_server(answers, before))
        .map(|(mut parsed, access)| {
            after_identity(&mut parsed, access.as_ref());
            parsed
        });
    Read { account, answer }
}

fn start(codex_home: &Path) -> Result<Session, String> {
    launch(codex_home).map_err(|failure| match failure {
        NotStarted::Missing => {
            "Codex CLI not found. Install or configure `codex`, then refresh Limits.".to_string()
        }
        NotStarted::Failed(message) => message,
    })
}

fn start_read(codex_home: &Path) -> Result<Session, ReadFailure> {
    Ok(launch(codex_home)?)
}

impl From<NotStarted> for ReadFailure {
    fn from(failure: NotStarted) -> Self {
        match failure {
            NotStarted::Missing => Self::NotInstalled,
            NotStarted::Failed(message) => Self::Unavailable(message),
        }
    }
}

pub(super) fn known_account(home: &Path) -> Option<crate::dto::LimitsAccountDto> {
    let before = crate::accounts::codex_store::metadata(&home.join(".codex")).ok()??;
    card_account(Some(&before), None)
}

fn launch(codex_home: &Path) -> Result<Session, NotStarted> {
    let binary = resolve_provider_cli(AgentId::Codex, "codex")
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "codex".to_string());
    let mut command: Command = AgentCli::new(binary).command();
    command
        .args(["app-server", "--stdio"])
        .env("CODEX_HOME", codex_home);
    Session::start(APP_SERVER, command)
}

fn query_app_server(
    session: Session,
    expected_codex_home: &Path,
    force: bool,
) -> Result<AppServerAnswers, String> {
    open_account(
        session,
        expected_codex_home,
        force,
        |codex_home, account, conversation| {
            let rate_limits = read_rate_limits(conversation)?;
            Ok(AppServerAnswers {
                codex_home,
                account,
                rate_limits,
            })
        },
    )
}

fn read_rate_limits(
    conversation: &mut Conversation,
) -> Result<super::codex::RateLimitsResponse, Failure> {
    conversation.request(Request::new(3, "account/rateLimits/read").when_missing(
        "Update Codex CLI to a version that supports subscription limits, then refresh here.",
    ))
}

fn open_account<T>(
    session: Session,
    expected_codex_home: &Path,
    force: bool,
    then: impl FnOnce(PathBuf, AccountReadResponse, &mut Conversation) -> Result<T, Failure>,
) -> Result<T, String> {
    session.run(
        |initialize: &InitializeResponse| {
            if crate::paths::equivalent(&initialize.codex_home, expected_codex_home) {
                return Ok(());
            }
            Err(format!(
                "Codex app-server used {}, expected {}.",
                initialize.codex_home.display(),
                expected_codex_home.display()
            ))
        },
        |initialize, conversation| {
            let account = conversation
                .request(Request::new(2, "account/read").params(json!({"refreshToken": force})))?;
            then(initialize.codex_home, account, conversation)
        },
    )
}

fn normalize_app_server(
    answers: AppServerAnswers,
    before: Option<Metadata>,
) -> Result<(Parsed, Option<CodexAccess>), ReadFailure> {
    let account = require_chatgpt(&answers.account)?;
    let email = account
        .email
        .as_deref()
        .map(str::trim)
        .filter(|email| !email.is_empty())
        .map(str::to_string);
    let mut reading = super::codex::parse_codex(&answers.rate_limits);
    reading.plan = account
        .plan_type
        .as_deref()
        .map(str::trim)
        .filter(|plan| !plan.is_empty())
        .map(str::to_string)
        .or(reading.plan);
    let (after, access) = crate::accounts::codex_store::metadata_and_access(&answers.codex_home)
        .map_err(ReadFailure::Unavailable)?
        .map_or((None, None), |(metadata, access)| (Some(metadata), access));
    if before.as_ref().map(|(id, _)| id) != after.as_ref().map(|(id, _)| id) {
        return Err(ReadFailure::AccountChanged);
    }
    let account = card_account(before.as_ref(), email.clone())
        .or_else(|| {
            email.as_deref().map(|email| crate::dto::LimitsAccountDto {
                legacy_id: None,
                id: format!("email:{}", email.to_lowercase()),
                label: Some(email.to_string()),
            })
        })
        .unwrap_or_else(|| crate::dto::LimitsAccountDto {
            legacy_id: None,
            id: super::DEFAULT_ACCOUNT.to_string(),
            label: None,
        });
    let parsed = Parsed {
        account: Some(account),
        reading,
    };
    Ok((parsed, access))
}

fn card_account(
    metadata: Option<&Metadata>,
    email: Option<String>,
) -> Option<crate::dto::LimitsAccountDto> {
    let (id, claims) = metadata?;
    Some(
        match crate::accounts::codex_store::claims_identity(claims) {
            Some(identity) => super::scoped_account(&identity, email),
            None => crate::dto::LimitsAccountDto {
                legacy_id: None,
                id: id.clone(),
                label: email,
            },
        },
    )
}

fn require_chatgpt(read: &AccountReadResponse) -> Result<&CodexAccount, ReadFailure> {
    let Some(account) = read.account.as_ref() else {
        return Err(ReadFailure::SignedOut);
    };
    match account.kind.as_str() {
        "chatgpt" => Ok(account),
        "apiKey" => Err(ReadFailure::Unsupported(Unsupported::ApiKey)),
        other => Err(ReadFailure::Unsupported(Unsupported::AccountType(
            other.to_string(),
        ))),
    }
}

#[cfg(test)]
mod tests;
