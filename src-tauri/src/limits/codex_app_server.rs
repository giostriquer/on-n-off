//! Bounded client for the official Codex app-server account APIs.

use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;

use super::Parsed;
use crate::accounts::native::CodexAccess;
use crate::cli::AgentCli;
use crate::cli_locate::resolve_provider_cli;
use crate::dto::{AgentId, ResetCreditOutcome};

const APP_SERVER_TIMEOUT: Duration = Duration::from_secs(30);
const STDOUT_LINE_LIMIT: usize = 1024 * 1024;
const MESSAGE_QUEUE_LIMIT: usize = 32;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryStage {
    Initialize,
    Account,
    RateLimits,
    ResetCredit,
}

#[derive(Debug, Deserialize)]
struct ConsumeResetCreditResponse {
    outcome: ResetCreditOutcome,
}

/// Why a reset was not spent: refused before the request was sent, or the session itself failed.
enum SpendFailure {
    Refused(String),
    Query(QueryError),
}

impl From<QueryError> for SpendFailure {
    fn from(error: QueryError) -> Self {
        Self::Query(error)
    }
}

#[derive(Debug)]
struct QueryError {
    stage: QueryStage,
    kind: QueryErrorKind,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryErrorKind {
    TransportClosed,
    Timeout,
    InvalidOutput,
    Protocol,
    Other,
}

#[derive(Debug)]
struct TransportError {
    kind: QueryErrorKind,
    message: String,
}

trait JsonLineTransport {
    fn send(&mut self, message: &Value) -> Result<(), TransportError>;
    fn receive(&mut self) -> Result<Value, TransportError>;
    /// Close the session and report how its process ended, when there is one.
    fn finish(&mut self) -> Option<ExitStatus> {
        None
    }
}

/// Spend one banked reset on the signed-in Codex account, which must still be `account_id`: Codex
/// applies a reset to whoever is signed in, so a card must never spend one on another account.
/// Blocking: runs a bounded app-server process. `idempotency_key` names one user attempt.
pub(super) fn consume_reset_credit(
    home: &Path,
    account_id: &str,
    idempotency_key: &str,
) -> Result<ResetCreditOutcome, String> {
    spend_reset_credit(
        &home.join(".codex"),
        account_id,
        idempotency_key,
        crate::accounts::native::codex_metadata,
        ProcessTransport::spawn,
    )
}

/// Every check before a reset is spent, in order: the native login is the card's account before
/// the app-server starts, the app-server is signed in with ChatGPT, and the native login is still the
/// card's account once the app-server has loaded it. Only then is the request sent.
fn spend_reset_credit<T: JsonLineTransport>(
    codex_home: &Path,
    account_id: &str,
    idempotency_key: &str,
    identity: impl Fn(&Path) -> Result<Option<(String, Value)>, String>,
    spawn: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<ResetCreditOutcome, String> {
    reset_target_matches(identity(codex_home)?, account_id)?;
    let mut transport = spawn(codex_home)?;
    let spent = spend_in_session(
        codex_home,
        account_id,
        idempotency_key,
        &identity,
        &mut transport,
    );
    let exit_status = transport.finish();
    spent.map_err(|failure| match failure {
        SpendFailure::Refused(message) => message,
        SpendFailure::Query(error) => classify_query_failure(error, exit_status),
    })
}

fn spend_in_session(
    codex_home: &Path,
    account_id: &str,
    idempotency_key: &str,
    identity: &impl Fn(&Path) -> Result<Option<(String, Value)>, String>,
    transport: &mut impl JsonLineTransport,
) -> Result<ResetCreditOutcome, SpendFailure> {
    let (_, account) = handshake(codex_home, false, transport)?;
    require_chatgpt(&account).map_err(|failure| {
        SpendFailure::Refused(match failure {
            AppServerFailure::SignedOut => {
                "Codex is not signed in, so there is no banked reset to use.".to_string()
            }
            _ => {
                "Banked resets belong to a ChatGPT sign-in, and Codex is not using one.".to_string()
            }
        })
    })?;
    // Codex may have loaded a different login than the one checked before it started.
    identity(codex_home)
        .and_then(|current| reset_target_matches(current, account_id))
        .map_err(SpendFailure::Refused)?;
    query_step(
        QueryStage::ResetCredit,
        transport.send(&serde_json::json!({
            "id": 3,
            "method": "account/rateLimitResetCredit/consume",
            "params": {"idempotencyKey": idempotency_key},
        })),
    )?;
    let response: ConsumeResetCreditResponse = receive_response(
        QueryStage::ResetCredit,
        transport,
        3,
        "account/rateLimitResetCredit/consume",
    )?;
    Ok(response.outcome)
}

/// A reset lands on whoever is signed in, so the native login must be the account the card names.
fn reset_target_matches(current: Option<(String, Value)>, account_id: &str) -> Result<(), String> {
    match current {
        Some((id, _)) if id == account_id => Ok(()),
        Some(_) => Err("The signed-in Codex account changed. Try again on its card.".to_string()),
        None => Err("on-n-off can't confirm which Codex account is signed in, so it won't spend a reset. Sign in again with `codex`.".to_string()),
    }
}

pub(super) fn read(home: &Path, force: bool) -> Result<Parsed, AppServerFailure> {
    read_with(home, force, ProcessTransport::spawn, |parsed, access| {
        backend_reads(parsed, access, LIVE_BACKEND, chrono::Utc::now());
    })
}

/// The endpoints the backend reads ask once the card's account is confirmed.
#[derive(Clone, Copy)]
pub(super) struct BackendUrls<'a> {
    pub(super) credit_usage: &'a str,
    pub(super) subscriptions: &'a str,
}

const LIVE_BACKEND: BackendUrls<'static> = BackendUrls {
    credit_usage: super::credits_spent::CODEX_CREDIT_USAGE_URL,
    subscriptions: super::renewal::CODEX_SUBSCRIPTIONS_URL,
};

/// The reads that take the confirmed card's access projection: what a workspace member spent, and
/// the subscription's term. Neither decides the read; each is no figure when it fails.
pub(super) fn backend_reads(
    parsed: &mut Parsed,
    access: Option<&CodexAccess>,
    urls: BackendUrls<'_>,
    now: chrono::DateTime<chrono::Utc>,
) {
    parsed.reading.credits_spent =
        super::credits_spent::signed_in(access, parsed, urls.credit_usage, now);
    parsed.reading.subscription =
        super::renewal::signed_in(access, parsed, urls.subscriptions, now);
}

/// `read`, with the app-server process and what runs once the card's account is confirmed (the
/// backend reads that take its access projection) replaceable for tests.
fn read_with<T: JsonLineTransport>(
    home: &Path,
    force: bool,
    spawn: impl FnOnce(&Path) -> Result<T, String>,
    after_identity: impl FnOnce(&mut Parsed, Option<&CodexAccess>),
) -> Result<Parsed, AppServerFailure> {
    let expected_codex_home = home.join(".codex");
    let before = crate::accounts::native::codex_metadata(&expected_codex_home)
        .map_err(AppServerFailure::Failed)?;
    let mut transport = spawn(&expected_codex_home).map_err(AppServerFailure::Failed)?;
    let session = query_app_server(&expected_codex_home, force, &mut transport);
    let exit_status = transport.finish();
    let session = session
        .map_err(|error| AppServerFailure::Failed(classify_query_failure(error, exit_status)))?;
    let (mut parsed, access) = normalize_app_server(session, before)?;
    // Only now is the card's account confirmed, and `access` was taken by that check.
    after_identity(&mut parsed, access.as_ref());
    Ok(parsed)
}

struct ProcessTransport {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Option<Receiver<Result<Value, TransportError>>>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
    deadline: Instant,
}

impl ProcessTransport {
    fn spawn(codex_home: &Path) -> Result<Self, String> {
        let binary = resolve_provider_cli(AgentId::Codex, "codex")
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "codex".to_string());
        let mut command: Command = AgentCli::new(binary).command();
        command
            .args(["app-server", "--stdio"])
            .env("CODEX_HOME", codex_home);
        Self::spawn_command(&mut command, APP_SERVER_TIMEOUT, STDOUT_LINE_LIMIT)
    }

    fn spawn_command(
        command: &mut Command,
        timeout: Duration,
        stdout_line_limit: usize,
    ) -> Result<Self, String> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    "Codex CLI not found. Install or configure `codex`, then refresh Limits."
                        .to_string()
                } else {
                    format!("Could not start Codex app-server: {error}")
                }
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Codex app-server stdin was not available.".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Codex app-server stdout was not available.".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "Codex app-server stderr was not available.".to_string())?;
        let (send, messages) = mpsc::sync_channel(MESSAGE_QUEUE_LIMIT);
        let stdout_thread = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = Vec::new();
            loop {
                let message = match read_bounded_line(&mut reader, &mut line, stdout_line_limit) {
                    Ok(None) => break,
                    Ok(Some(true)) => Err(TransportError {
                        kind: QueryErrorKind::InvalidOutput,
                        message: format!(
                            "Codex app-server output line exceeded {stdout_line_limit} bytes."
                        ),
                    }),
                    Ok(Some(false)) => {
                        serde_json::from_slice::<Value>(&line).map_err(|error| TransportError {
                            kind: QueryErrorKind::InvalidOutput,
                            message: format!("Codex app-server returned invalid JSON: {error}"),
                        })
                    }
                    Err(error) => Err(TransportError {
                        kind: QueryErrorKind::Other,
                        message: format!("Could not read Codex app-server output: {error}"),
                    }),
                };
                if send.send(message).is_err() {
                    break;
                }
            }
        });
        let stderr_thread = thread::spawn(move || {
            let _ = io::copy(&mut BufReader::new(stderr), &mut io::sink());
        });
        Ok(Self {
            child,
            stdin: Some(stdin),
            messages: Some(messages),
            stdout_thread: Some(stdout_thread),
            stderr_thread: Some(stderr_thread),
            deadline: Instant::now() + timeout,
        })
    }
}

impl JsonLineTransport for ProcessTransport {
    fn finish(&mut self) -> Option<ExitStatus> {
        self.stdin.take();
        self.messages.take();
        let mut status = None;
        while Instant::now() < self.deadline {
            match self.child.try_wait() {
                Ok(Some(exit_status)) => {
                    status = Some(exit_status);
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(_) => break,
            }
        }
        let detach_drainers = status.is_none();
        if detach_drainers {
            let _ = self.child.kill();
            status = self.child.wait().ok();
        }
        if detach_drainers {
            self.stdout_thread.take();
            self.stderr_thread.take();
            return status;
        }
        if let Some(thread) = self.stdout_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
        status
    }

    fn send(&mut self, message: &Value) -> Result<(), TransportError> {
        let stdin = self.stdin.as_mut().ok_or_else(|| TransportError {
            kind: QueryErrorKind::TransportClosed,
            message: "Codex app-server input is closed.".to_string(),
        })?;
        serde_json::to_writer(&mut *stdin, message).map_err(|error| TransportError {
            kind: QueryErrorKind::Other,
            message: format!("Could not encode Codex app-server request: {error}"),
        })?;
        stdin
            .write_all(b"\n")
            .and_then(|()| stdin.flush())
            .map_err(|error| TransportError {
                kind: if matches!(
                    error.kind(),
                    io::ErrorKind::BrokenPipe
                        | io::ErrorKind::ConnectionAborted
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::NotConnected
                ) {
                    QueryErrorKind::TransportClosed
                } else {
                    QueryErrorKind::Other
                },
                message: format!("Could not write to Codex app-server: {error}"),
            })
    }

    fn receive(&mut self) -> Result<Value, TransportError> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(TransportError {
                kind: QueryErrorKind::Timeout,
                message: "Codex app-server timed out.".to_string(),
            });
        }
        self.messages
            .as_ref()
            .ok_or_else(|| TransportError {
                kind: QueryErrorKind::TransportClosed,
                message: "Codex app-server output is closed.".to_string(),
            })?
            .recv_timeout(remaining)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => TransportError {
                    kind: QueryErrorKind::Timeout,
                    message: "Codex app-server timed out.".to_string(),
                },
                mpsc::RecvTimeoutError::Disconnected => TransportError {
                    kind: QueryErrorKind::TransportClosed,
                    message: "Codex app-server closed before returning limits.".to_string(),
                },
            })?
    }
}

impl Drop for ProcessTransport {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn read_bounded_line(
    reader: &mut impl BufRead,
    output: &mut Vec<u8>,
    limit: usize,
) -> io::Result<Option<bool>> {
    output.clear();
    let mut oversized = false;
    let mut saw_bytes = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(saw_bytes.then_some(oversized));
        }
        saw_bytes = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        if !oversized {
            let remaining = limit.saturating_sub(output.len());
            let copied = consumed.min(remaining);
            output.extend_from_slice(&available[..copied]);
            oversized = copied < consumed;
        }
        reader.consume(consumed);
        if newline.is_some() {
            return Ok(Some(oversized));
        }
    }
}

fn query_app_server(
    expected_codex_home: &Path,
    force: bool,
    transport: &mut impl JsonLineTransport,
) -> Result<AppServerResult, QueryError> {
    let (codex_home, account) = handshake(expected_codex_home, force, transport)?;
    query_step(
        QueryStage::RateLimits,
        transport
            .send(&serde_json::json!({"id": 3, "method": "account/rateLimits/read", "params": {}})),
    )?;
    let rate_limits = receive_response(
        QueryStage::RateLimits,
        transport,
        3,
        "account/rateLimits/read",
    )?;
    Ok(AppServerResult {
        codex_home,
        account,
        rate_limits,
    })
}

/// `initialize` against the expected home, then `account/read`: the start of every app-server call.
fn handshake(
    expected_codex_home: &Path,
    force: bool,
    transport: &mut impl JsonLineTransport,
) -> Result<(PathBuf, AccountReadResponse), QueryError> {
    query_step(
        QueryStage::Initialize,
        transport.send(&serde_json::json!({
            "id": 1,
            "method": "initialize",
            "params": {"clientInfo": {
                "name": "on_n_off",
                "title": "on-n-off",
                "version": env!("CARGO_PKG_VERSION"),
            }},
        })),
    )?;
    let initialize: InitializeResponse =
        receive_response(QueryStage::Initialize, transport, 1, "initialize")?;
    let codex_home = initialize.codex_home;
    if !paths_are_equivalent(&codex_home, expected_codex_home) {
        return Err(QueryError {
            stage: QueryStage::Initialize,
            kind: QueryErrorKind::Protocol,
            message: format!(
                "Codex app-server used {}, expected {}.",
                codex_home.display(),
                expected_codex_home.display()
            ),
        });
    }

    query_step(
        QueryStage::Account,
        transport.send(&serde_json::json!({"method": "initialized", "params": {}})),
    )?;
    query_step(
        QueryStage::Account,
        transport.send(&serde_json::json!({
            "id": 2,
            "method": "account/read",
            "params": {"refreshToken": force},
        })),
    )?;
    let account = receive_response(QueryStage::Account, transport, 2, "account/read")?;
    Ok((codex_home, account))
}

fn query_step<T>(stage: QueryStage, result: Result<T, TransportError>) -> Result<T, QueryError> {
    result.map_err(|error| QueryError {
        stage,
        kind: error.kind,
        message: error.message,
    })
}

fn classify_query_failure(error: QueryError, exit_status: Option<ExitStatus>) -> String {
    if error.stage == QueryStage::Initialize
        && error.kind == QueryErrorKind::TransportClosed
        && exit_status.is_some_and(|status| !status.success())
    {
        return "Codex CLI exited before app-server initialized. This version may not support `codex app-server`; update Codex CLI, then refresh here."
            .to_string();
    }
    error.message
}

fn receive_response<T: DeserializeOwned>(
    stage: QueryStage,
    transport: &mut impl JsonLineTransport,
    expected_id: u64,
    method: &str,
) -> Result<T, QueryError> {
    loop {
        let message = query_step(stage, transport.receive())?;
        if message.get("id").and_then(Value::as_u64) == Some(expected_id) {
            return response_result(&message, method).map_err(|message| QueryError {
                stage,
                kind: QueryErrorKind::Protocol,
                message,
            });
        }
    }
}

fn response_result<T: DeserializeOwned>(message: &Value, method: &str) -> Result<T, String> {
    if let Some(result) = message.get("result") {
        return serde_json::from_value(result.clone()).map_err(|error| {
            format!("Codex app-server `{method}` returned a malformed response: {error}")
        });
    }
    let error = message.get("error");
    let code = error
        .and_then(|error| error.get("code"))
        .and_then(Value::as_i64);
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("unknown protocol error");
    let method_missing = code == Some(-32601);
    let code = code.map_or_else(String::new, |code| format!(" (code {code})"));
    let guidance = match method {
        "account/rateLimits/read" if method_missing => {
            " Update Codex CLI to a version that supports subscription limits, then refresh here."
        }
        "account/rateLimitResetCredit/consume" if method_missing => {
            " Update Codex CLI to a version that supports banked resets, then try again."
        }
        _ => "",
    };
    Err(format!(
        "Codex app-server `{method}` failed{code}: {message}.{guidance}"
    ))
}

/// The card for the account app-server read, once the native login is confirmed to be the account
/// captured before it started, with that login's access projection taken in the same read of the
/// native store as the check.
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
    // One read of the native store confirms the account and takes the access projection the term
    // read needs for every card, and the spending read for a workspace plan (`renewal::signed_in`,
    // `credits_spent::signed_in`).
    let (after, access) = crate::accounts::native::codex_metadata_and_access(&session.codex_home)
        .map_err(AppServerFailure::Failed)?
        .map_or((None, None), |(metadata, access)| (Some(metadata), access));
    if before.as_ref().map(|(id, _)| id) != after.as_ref().map(|(id, _)| id) {
        return Err(AppServerFailure::Failed(
            "The signed-in account changed while reading usage. Retry after sign-in finishes."
                .into(),
        ));
    }
    // The identity captured before spawning owns this response. A token refresh may
    // change claims, but a different user or workspace must never inherit its usage.
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

/// Subscription limits and banked resets exist only for a ChatGPT login.
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

fn paths_are_equivalent(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    path_values_are_equivalent(&left, &right)
}

#[cfg(windows)]
fn path_values_are_equivalent(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/")
        .eq_ignore_ascii_case(
            &right
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .replace('\\', "/"),
        )
}

#[cfg(not(windows))]
fn path_values_are_equivalent(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(test)]
mod tests;
