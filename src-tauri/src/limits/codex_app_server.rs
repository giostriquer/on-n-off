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
use crate::cli::AgentCli;
use crate::cli_locate::resolve_provider_cli;
use crate::dto::AgentId;

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
}

pub(super) fn read(home: &Path, force: bool) -> Result<Parsed, AppServerFailure> {
    let expected_codex_home = home.join(".codex");
    let mut transport =
        ProcessTransport::spawn(&expected_codex_home).map_err(AppServerFailure::Failed)?;
    let session = query_app_server(&expected_codex_home, force, &mut transport);
    let exit_status = transport.finish();
    let session = session.map_err(|error| classify_query_failure(error, exit_status))?;
    normalize_app_server(session)
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
}

impl JsonLineTransport for ProcessTransport {
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

fn query_step<T>(stage: QueryStage, result: Result<T, TransportError>) -> Result<T, QueryError> {
    result.map_err(|error| QueryError {
        stage,
        kind: error.kind,
        message: error.message,
    })
}

fn classify_query_failure(error: QueryError, exit_status: Option<ExitStatus>) -> AppServerFailure {
    if error.stage == QueryStage::Initialize
        && error.kind == QueryErrorKind::TransportClosed
        && exit_status.is_some_and(|status| !status.success())
    {
        return AppServerFailure::Failed(
            "Codex CLI exited before app-server initialized. This version may not support `codex app-server`; update Codex CLI, then refresh here."
                .to_string(),
        );
    }
    AppServerFailure::Failed(error.message)
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
    let guidance = if method == "account/rateLimits/read" && method_missing {
        " Update Codex CLI to a version that supports subscription limits, then refresh here."
    } else {
        ""
    };
    Err(format!(
        "Codex app-server `{method}` failed{code}: {message}.{guidance}"
    ))
}

fn normalize_app_server(session: AppServerResult) -> Result<Parsed, AppServerFailure> {
    let Some(account) = session.account.account else {
        return Err(AppServerFailure::SignedOut);
    };
    match account.kind.as_str() {
        "chatgpt" => {}
        "apiKey" => {
            return Err(AppServerFailure::Unsupported(
                "Codex is signed in with an API key, which has no subscription limits.".to_string(),
            ))
        }
        other => {
            return Err(AppServerFailure::Unsupported(format!(
                "Codex account type `{other}` has no subscription limits to show."
            )))
        }
    }
    let email = account
        .email
        .as_deref()
        .map(str::trim)
        .filter(|email| !email.is_empty())
        .map(str::to_string);
    let account_id = read_account_id(&session.codex_home)
        .or_else(|| {
            email
                .as_deref()
                .map(|email| format!("email:{}", email.to_lowercase()))
        })
        .unwrap_or_else(|| super::DEFAULT_ACCOUNT.to_string());
    let mut parsed = super::codex::parse_codex(&session.rate_limits);
    parsed.account = Some(crate::dto::LimitsAccountDto {
        id: account_id,
        label: email,
    });
    parsed.plan = account
        .plan_type
        .as_deref()
        .map(str::trim)
        .filter(|plan| !plan.is_empty())
        .map(str::to_string)
        .or(parsed.plan);
    Ok(parsed)
}

fn read_account_id(codex_home: &Path) -> Option<String> {
    #[derive(Deserialize)]
    struct AuthMetadata {
        tokens: Option<TokenMetadata>,
    }

    #[derive(Deserialize)]
    struct TokenMetadata {
        account_id: Option<String>,
    }

    let file = std::fs::File::open(codex_home.join("auth.json")).ok()?;
    let metadata: AuthMetadata = serde_json::from_reader(file).ok()?;
    let account_id = metadata.tokens?.account_id?;
    let account_id = account_id.trim();
    (!account_id.is_empty()).then(|| account_id.to_string())
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
