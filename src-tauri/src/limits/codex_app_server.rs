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
        if status.is_none() {
            let _ = self.child.kill();
            status = self.child.wait().ok();
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
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;

    fn typed<T: DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value).unwrap()
    }

    struct FakeTransport {
        received: VecDeque<Value>,
        sent: Vec<Value>,
    }

    struct RefreshOrderingTransport {
        received: VecDeque<Value>,
        account_response_consumed: bool,
    }

    impl JsonLineTransport for RefreshOrderingTransport {
        fn send(&mut self, message: &Value) -> Result<(), TransportError> {
            if message.get("method").and_then(Value::as_str) == Some("account/rateLimits/read")
                && !self.account_response_consumed
            {
                return Err(TransportError {
                    kind: QueryErrorKind::Other,
                    message: "rate limits requested before account refresh completed".to_string(),
                });
            }
            Ok(())
        }

        fn receive(&mut self) -> Result<Value, TransportError> {
            let message = self.received.pop_front().ok_or_else(|| TransportError {
                kind: QueryErrorKind::Other,
                message: "no more app-server messages".to_string(),
            })?;
            if message.get("id").and_then(Value::as_u64) == Some(2) {
                self.account_response_consumed = true;
            }
            Ok(message)
        }
    }

    impl JsonLineTransport for FakeTransport {
        fn send(&mut self, message: &Value) -> Result<(), TransportError> {
            self.sent.push(message.clone());
            Ok(())
        }

        fn receive(&mut self) -> Result<Value, TransportError> {
            self.received.pop_front().ok_or_else(|| TransportError {
                kind: QueryErrorKind::Other,
                message: "no more app-server messages".to_string(),
            })
        }
    }

    #[test]
    fn completes_the_handshake_and_matches_account_and_rate_limit_responses_by_id() {
        let codex_home = PathBuf::from("/fixture/.codex");
        let account = json!({
            "account": {"type": "chatgpt", "email": "me@example.com", "planType": "pro"},
            "requiresOpenaiAuth": true
        });
        let rate_limits = json!({
            "rateLimits": {
                "limitId": "codex",
                "primary": {"usedPercent": 42, "windowDurationMins": 10080},
                "planType": "pro"
            }
        });
        let mut transport = FakeTransport {
            received: VecDeque::from([
                json!({"method": "remoteControl/status/changed", "params": {"status": "disabled"}}),
                json!({"id": 1, "result": {
                    "userAgent": "on_n_off/0.148.0",
                    "codexHome": "/fixture/.codex",
                    "platformFamily": "unix",
                    "platformOs": "macos"
                }}),
                json!({"id": 2, "result": account.clone()}),
                json!({"id": 3, "result": rate_limits.clone()}),
            ]),
            sent: Vec::new(),
        };

        let result = query_app_server(&codex_home, true, &mut transport).unwrap();

        assert_eq!(result.codex_home, codex_home);
        assert_eq!(result.account, typed(account));
        assert_eq!(result.rate_limits, typed(rate_limits));
        assert_eq!(
            transport.sent,
            [
                json!({"id": 1, "method": "initialize", "params": {"clientInfo": {
                    "name": "on_n_off", "title": "on-n-off", "version": env!("CARGO_PKG_VERSION")
                }}}),
                json!({"method": "initialized", "params": {}}),
                json!({"id": 2, "method": "account/read", "params": {"refreshToken": true}}),
                json!({"id": 3, "method": "account/rateLimits/read", "params": {}}),
            ]
        );
    }

    #[test]
    fn ordinary_reads_leave_forced_refresh_disabled() {
        let codex_home = PathBuf::from("/fixture/.codex");
        let mut transport = FakeTransport {
            received: VecDeque::from([
                json!({"id": 1, "result": {
                    "userAgent": "on_n_off/0.148.0",
                    "codexHome": "/fixture/.codex",
                    "platformFamily": "unix",
                    "platformOs": "macos"
                }}),
                json!({"id": 2, "result": {
                    "account": {"type": "chatgpt", "email": "me@example.com", "planType": "pro"},
                    "requiresOpenaiAuth": true
                }}),
                json!({"id": 3, "result": {"rateLimits": {}}}),
            ]),
            sent: Vec::new(),
        };

        query_app_server(&codex_home, false, &mut transport).unwrap();

        assert_eq!(transport.sent[2]["params"]["refreshToken"], false);
    }

    #[test]
    fn rate_limits_wait_for_account_refresh_to_complete() {
        let codex_home = PathBuf::from("/fixture/.codex");
        let mut transport = RefreshOrderingTransport {
            received: VecDeque::from([
                json!({"id": 1, "result": {
                    "userAgent": "on_n_off/0.148.0",
                    "codexHome": "/fixture/.codex",
                    "platformFamily": "unix",
                    "platformOs": "macos"
                }}),
                json!({"id": 2, "result": {
                    "account": {"type": "chatgpt", "email": "me@example.com", "planType": "pro"},
                    "requiresOpenaiAuth": true
                }}),
                json!({"id": 3, "result": {"rateLimits": {}}}),
            ]),
            account_response_consumed: false,
        };

        let result = query_app_server(&codex_home, true, &mut transport);

        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn protocol_errors_are_returned_instead_of_becoming_empty_limits() {
        let codex_home = PathBuf::from("/fixture/.codex");
        let mut transport = FakeTransport {
            received: VecDeque::from([
                json!({"id": 1, "result": {
                    "userAgent": "on_n_off/0.120.0",
                    "codexHome": "/fixture/.codex",
                    "platformFamily": "unix",
                    "platformOs": "macos"
                }}),
                json!({"id": 2, "result": {
                    "account": {"type": "chatgpt", "email": "me@example.com", "planType": "pro"},
                    "requiresOpenaiAuth": true
                }}),
                json!({"id": 3, "error": {"code": -32601, "message": "Method not found"}}),
            ]),
            sent: Vec::new(),
        };

        let error = query_app_server(&codex_home, false, &mut transport)
            .unwrap_err()
            .message;

        assert!(error.contains("account/rateLimits/read"), "{error}");
        assert!(error.contains("Update Codex CLI"), "{error}");
        assert!(error.contains("Method not found"), "{error}");
    }

    #[test]
    fn malformed_rate_limit_payload_is_a_failure_instead_of_an_empty_success() {
        let codex_home = PathBuf::from("/fixture/.codex");
        let mut transport = FakeTransport {
            received: VecDeque::from([
                json!({"id": 1, "result": {"codexHome": "/fixture/.codex"}}),
                json!({"id": 2, "result": {
                    "account": {"type": "chatgpt", "email": "me@example.com", "planType": "pro"}
                }}),
                json!({"id": 3, "result": {"unexpected": true}}),
            ]),
            sent: Vec::new(),
        };

        let error = query_app_server(&codex_home, false, &mut transport)
            .unwrap_err()
            .message;

        assert!(error.contains("rateLimits"), "{error}");
        assert!(error.contains("malformed response"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn accepts_a_symlinked_codex_home_that_resolves_to_the_expected_directory() {
        use std::os::unix::fs::symlink;

        let root = crate::paths::scratch_dir("codex-app-server-home-link");
        let expected = root.join("real");
        let alias = root.join("alias");
        std::fs::create_dir_all(&expected).unwrap();
        symlink(&expected, &alias).unwrap();
        let mut transport = FakeTransport {
            received: VecDeque::from([
                json!({"id": 1, "result": {
                    "userAgent": "on_n_off/0.148.0",
                    "codexHome": alias,
                    "platformFamily": "unix",
                    "platformOs": "macos"
                }}),
                json!({"id": 2, "result": {"account": null, "requiresOpenaiAuth": true}}),
                json!({"id": 3, "result": {"rateLimits": {}}}),
            ]),
            sent: Vec::new(),
        };

        let result = query_app_server(&expected, false, &mut transport);

        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn process_transport_times_out_and_stops_the_child() {
        let root = crate::paths::scratch_dir("codex-app-server-timeout");
        let cli = crate::cli_stub::CliStub::new("codex").sleep(5).cli(&root);
        let mut command = cli.command();
        let started = Instant::now();
        let mut transport =
            ProcessTransport::spawn_command(&mut command, Duration::from_millis(100), 1024)
                .unwrap();

        let error = transport.receive().unwrap_err().message;
        transport.finish();

        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(transport.child.try_wait().unwrap().is_some());
    }

    #[test]
    fn process_transport_rejects_an_oversized_stdout_line() {
        let root = crate::paths::scratch_dir("codex-app-server-output-limit");
        let output = "x".repeat(128);
        let cli = crate::cli_stub::CliStub::new("codex")
            .stdout(&output)
            .cli(&root);
        let mut command = cli.command();
        let mut transport =
            ProcessTransport::spawn_command(&mut command, Duration::from_secs(10), 64).unwrap();

        let error = transport.receive().unwrap_err().message;
        transport.finish();

        assert!(error.contains("exceeded 64 bytes"), "{error}");
    }

    #[test]
    fn process_transport_does_not_surface_stderr_content() {
        let root = crate::paths::scratch_dir("codex-app-server-stderr");
        let cli = crate::cli_stub::CliStub::new("codex")
            .stdout("not-json")
            .stderr("sensitive-provider-diagnostic")
            .cli(&root);
        let mut command = cli.command();
        let mut transport =
            ProcessTransport::spawn_command(&mut command, Duration::from_secs(10), 1024).unwrap();

        let error = transport.receive().unwrap_err().message;
        transport.finish();

        assert!(error.contains("invalid JSON"), "{error}");
        assert!(!error.contains("sensitive-provider-diagnostic"), "{error}");
    }

    #[test]
    fn early_nonzero_exit_explains_that_the_cli_may_need_an_update() {
        let root = crate::paths::scratch_dir("codex-app-server-old-cli");
        let cli = crate::cli_stub::CliStub::new("codex").exit(2).cli(&root);
        let mut command = cli.command();
        let mut transport =
            ProcessTransport::spawn_command(&mut command, Duration::from_secs(10), 1024).unwrap();

        let error = query_app_server(&root, false, &mut transport).unwrap_err();
        let status = transport.finish();
        let failure = classify_query_failure(error, status);

        assert!(matches!(
            failure,
            AppServerFailure::Failed(message)
                if message.contains("may not support `codex app-server`")
                    && message.contains("update Codex CLI")
        ));
    }

    #[test]
    fn early_broken_pipe_and_nonzero_exit_give_the_same_old_cli_guidance() {
        let root = crate::paths::scratch_dir("codex-app-server-old-cli-write");
        let cli = crate::cli_stub::CliStub::new("codex").exit(2).cli(&root);
        let status = cli.command().status().unwrap();
        let error = QueryError {
            stage: QueryStage::Initialize,
            kind: QueryErrorKind::TransportClosed,
            message: "Could not write to Codex app-server: Broken pipe".to_string(),
        };

        let failure = classify_query_failure(error, Some(status));

        assert!(matches!(
            failure,
            AppServerFailure::Failed(message)
                if message.contains("may not support `codex app-server`")
                    && message.contains("update Codex CLI")
        ));
    }

    #[test]
    fn nonzero_exit_does_not_hide_typed_timeout_or_invalid_output_errors() {
        for (kind, expected) in [
            (QueryErrorKind::Timeout, "timed out"),
            (QueryErrorKind::InvalidOutput, "invalid JSON"),
        ] {
            let root = crate::paths::scratch_dir("codex-app-server-specific-error");
            let cli = crate::cli_stub::CliStub::new("codex").exit(2).cli(&root);
            let status = cli.command().status().unwrap();
            let error = QueryError {
                stage: QueryStage::Initialize,
                kind,
                message: format!("Codex app-server {expected}."),
            };

            let failure = classify_query_failure(error, Some(status));

            assert_eq!(
                failure,
                AppServerFailure::Failed(format!("Codex app-server {expected}."))
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_home_comparison_is_case_and_separator_insensitive() {
        assert!(path_values_are_equivalent(
            Path::new(r"C:\Users\Me\.codex"),
            Path::new("c:/users/me/.codex")
        ));
    }

    #[test]
    fn normalizes_the_chatgpt_account_and_rate_limits_without_reading_a_token() {
        let codex_home = crate::paths::scratch_dir("codex-app-server-normalize");
        std::fs::write(
            codex_home.join("auth.json"),
            r#"{"tokens":{"account_id":"acct-1"}}"#,
        )
        .unwrap();
        let parsed = normalize_app_server(AppServerResult {
            codex_home,
            account: typed(json!({
                "account": {
                    "type": "chatgpt",
                    "email": "Me@Example.com",
                    "planType": "pro"
                },
                "requiresOpenaiAuth": true
            })),
            rate_limits: typed(json!({
                "rateLimits": {
                    "limitId": "codex",
                    "primary": {
                        "usedPercent": 42,
                        "windowDurationMins": 10080,
                        "resetsAt": 1787838960
                    },
                    "secondary": null,
                    "credits": {"hasCredits": false, "unlimited": false, "balance": "0"},
                    "planType": "pro"
                }
            })),
        })
        .unwrap();

        assert_eq!(
            parsed.account,
            Some(crate::dto::LimitsAccountDto {
                id: "acct-1".to_string(),
                label: Some("Me@Example.com".to_string()),
            })
        );
        assert_eq!(parsed.plan.as_deref(), Some("pro"));
        assert_eq!(parsed.windows.len(), 1);
        assert_eq!(parsed.windows[0].used_percent, 42.0);
    }

    #[test]
    fn falls_back_to_a_normalized_email_identity_when_codex_has_no_account_id() {
        let codex_home = crate::paths::scratch_dir("codex-app-server-email");
        let parsed = normalize_app_server(AppServerResult {
            codex_home,
            account: typed(json!({
                "account": {
                    "type": "chatgpt",
                    "email": " Me@Example.com ",
                    "planType": "plus"
                },
                "requiresOpenaiAuth": true
            })),
            rate_limits: typed(json!({"rateLimits": {"planType": "plus"}})),
        })
        .unwrap();

        assert_eq!(
            parsed.account,
            Some(crate::dto::LimitsAccountDto {
                id: "email:me@example.com".to_string(),
                label: Some("Me@Example.com".to_string()),
            })
        );
    }

    #[test]
    fn api_key_accounts_are_explicitly_unsupported() {
        let error = normalize_app_server(AppServerResult {
            codex_home: crate::paths::scratch_dir("codex-app-server-api-key"),
            account: typed(json!({
                "account": {"type": "apiKey"},
                "requiresOpenaiAuth": false
            })),
            rate_limits: typed(json!({"rateLimits": {}})),
        })
        .unwrap_err();

        assert!(matches!(
            error,
            AppServerFailure::Unsupported(message) if message.contains("API key")
        ));
    }
}
