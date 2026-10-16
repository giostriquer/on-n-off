// Muse subscription limits, read the way Muse exposes them: `muse serve`
// speaks MSP over stdio, and its experimental `usage/read` reports the last
// subscription usage the host observed from a provider frame. on-n-off opens
// no Muse credential and never starts a model turn to poll: when the fresh
// host has observed nothing, the card says so and keeps its account so the
// remembered reading stands in.
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;

use super::pipeline::finish;
use super::Parsed;
use crate::cli::AgentCli;
use crate::cli_locate::resolve_provider_cli;
use crate::dto::{
    AgentId, LimitWindowDto, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto,
    Reading,
};

const SERVE_TIMEOUT: Duration = Duration::from_secs(30);
const STDOUT_LINE_LIMIT: usize = 1024 * 1024;
const MESSAGE_QUEUE_LIMIT: usize = 32;

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
struct ServeSession {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryStage {
    Initialize,
    Account,
    Usage,
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
    fn finish(&mut self) -> Option<ExitStatus> {
        None
    }
}

fn query_serve(
    expected_data: &Path,
    transport: &mut impl JsonLineTransport,
) -> Result<ServeSession, QueryError> {
    query_step(
        QueryStage::Initialize,
        transport.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "on_n_off",
                    "title": "on-n-off",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {"experimentalApi": true, "userInputDialogs": false},
            },
        })),
    )?;
    let initialize: InitializeResult =
        receive_response(QueryStage::Initialize, transport, 1, "initialize")?;
    if !crate::paths::equivalent(&initialize.muse_home, expected_data) {
        return Err(QueryError {
            stage: QueryStage::Initialize,
            kind: QueryErrorKind::Protocol,
            message: format!(
                "Muse serve used {}, expected {}.",
                initialize.muse_home.display(),
                expected_data.display()
            ),
        });
    }
    if !initialize.experimental_api {
        return Err(QueryError {
            stage: QueryStage::Initialize,
            kind: QueryErrorKind::Protocol,
            message: "Update Muse: this version cannot report subscription usage to on-n-off."
                .to_string(),
        });
    }

    query_step(
        QueryStage::Account,
        transport
            .send(&serde_json::json!({"jsonrpc": "2.0", "method": "initialized", "params": {}})),
    )?;
    query_step(
        QueryStage::Account,
        transport.send(
            &serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "account/read", "params": {}}),
        ),
    )?;
    let account: AccountState =
        receive_response(QueryStage::Account, transport, 2, "account/read")?;

    // Signed-out and API-key hosts have no usage to ask for; normalize maps
    // those states, and a usage/read would only add a failing step.
    if account.state != "accountLogin" {
        return Ok(ServeSession {
            account,
            usage: None,
        });
    }

    query_step(
        QueryStage::Usage,
        transport.send(
            &serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "usage/read", "params": {}}),
        ),
    )?;
    let usage_read: UsageReadResult =
        receive_response(QueryStage::Usage, transport, 3, "usage/read")?;

    query_step(
        QueryStage::Account,
        transport.send(
            &serde_json::json!({"jsonrpc": "2.0", "id": 4, "method": "account/read", "params": {}}),
        ),
    )?;
    let after: AccountState = receive_response(QueryStage::Account, transport, 4, "account/read")?;
    if after != account {
        return Err(QueryError {
            stage: QueryStage::Account,
            kind: QueryErrorKind::Protocol,
            message: "The signed-in Muse account changed while its usage was read.".to_string(),
        });
    }

    Ok(ServeSession {
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

fn query_step<T>(stage: QueryStage, result: Result<T, TransportError>) -> Result<T, QueryError> {
    result.map_err(|error| QueryError {
        stage,
        kind: error.kind,
        message: error.message,
    })
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
            format!("Muse serve `{method}` returned a malformed response: {error}")
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
    let guidance = if code == Some(-32601) && method == "usage/read" {
        " Update Muse to a version that reports subscription usage, then refresh here."
    } else {
        ""
    };
    let code = code.map_or_else(String::new, |code| format!(" (code {code})"));
    Err(format!(
        "Muse serve `{method}` failed{code}: {message}.{guidance}"
    ))
}

fn normalize(session: ServeSession) -> Result<Parsed, ServeFailure> {
    let account = &session.account;
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
    let Some(usage) = session.usage else {
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
    finish_read(read_with(home, ProcessTransport::spawn))
}

fn read_with<T: JsonLineTransport>(
    home: &Path,
    spawn: impl FnOnce(&Path, &Path, &Path) -> Result<T, ServeFailure>,
) -> Result<Parsed, ServeFailure> {
    let config = crate::paths::muse_config_for(home);
    let data = crate::paths::muse_data_for(home);
    let mut transport = spawn(home, &config, &data)?;
    let session = query_serve(&data, &mut transport);
    let exit_status = transport.finish();
    let session = session
        .map_err(|error| ServeFailure::Failed(classify_query_failure(error, exit_status)))?;
    normalize(session)
}

fn classify_query_failure(error: QueryError, exit_status: Option<ExitStatus>) -> String {
    if error.stage == QueryStage::Initialize
        && error.kind == QueryErrorKind::TransportClosed
        && exit_status.is_some_and(|status| !status.success())
    {
        return "Muse CLI exited before `muse serve` initialized. This version may not support `muse serve`; update Muse Code, then refresh here."
            .to_string();
    }
    error.message
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
    fn spawn(home: &Path, config: &Path, data: &Path) -> Result<Self, ServeFailure> {
        let binary = resolve_provider_cli(AgentId::Muse, "muse")
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "muse".to_string());
        let mut command: Command = AgentCli::new(binary).command();
        // Memory-only sessions and no shell/write tools: the read only
        // asks account/read and usage/read, and must not persist a session
        // or run any of the user's customizations.
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
        Self::spawn_command(&mut command, SERVE_TIMEOUT, STDOUT_LINE_LIMIT)
    }

    fn spawn_command(
        command: &mut Command,
        timeout: Duration,
        stdout_line_limit: usize,
    ) -> Result<Self, ServeFailure> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    ServeFailure::NotInstalled
                } else {
                    ServeFailure::Failed(format!("Could not start Muse serve: {error}"))
                }
            })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            ServeFailure::Failed("Muse serve stdin was not available.".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            ServeFailure::Failed("Muse serve stdout was not available.".to_string())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            ServeFailure::Failed("Muse serve stderr was not available.".to_string())
        })?;
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
                            "Muse serve output line exceeded {stdout_line_limit} bytes."
                        ),
                    }),
                    Ok(Some(false)) => {
                        serde_json::from_slice::<Value>(&line).map_err(|error| TransportError {
                            kind: QueryErrorKind::InvalidOutput,
                            message: format!("Muse serve returned invalid JSON: {error}"),
                        })
                    }
                    Err(error) => Err(TransportError {
                        kind: QueryErrorKind::Other,
                        message: format!("Could not read Muse serve output: {error}"),
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
            message: "Muse serve input is closed.".to_string(),
        })?;
        serde_json::to_writer(&mut *stdin, message).map_err(|error| TransportError {
            kind: QueryErrorKind::Other,
            message: format!("Could not encode Muse serve request: {error}"),
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
                message: format!("Could not write to Muse serve: {error}"),
            })
    }

    fn receive(&mut self) -> Result<Value, TransportError> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(TransportError {
                kind: QueryErrorKind::Timeout,
                message: "Muse serve timed out.".to_string(),
            });
        }
        self.messages
            .as_ref()
            .ok_or_else(|| TransportError {
                kind: QueryErrorKind::TransportClosed,
                message: "Muse serve output is closed.".to_string(),
            })?
            .recv_timeout(remaining)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => TransportError {
                    kind: QueryErrorKind::Timeout,
                    message: "Muse serve timed out.".to_string(),
                },
                mpsc::RecvTimeoutError::Disconnected => TransportError {
                    kind: QueryErrorKind::TransportClosed,
                    message: "Muse serve closed before returning limits.".to_string(),
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

#[cfg(test)]
mod tests;
