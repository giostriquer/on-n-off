use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde_json::{json, Map, Value};

const DEADLINE: Duration = Duration::from_secs(30);
const LINE_LIMIT: usize = 1024 * 1024;
const MESSAGE_QUEUE_LIMIT: usize = 32;
const METHOD_NOT_FOUND: i64 = -32601;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Peer {
    name: &'static str,
    exited_early: &'static str,
    jsonrpc: bool,
    timeout: Duration,
    line_limit: usize,
}

impl Peer {
    pub(crate) const fn jsonrpc_lite(name: &'static str, exited_early: &'static str) -> Self {
        Self {
            name,
            exited_early,
            jsonrpc: false,
            timeout: DEADLINE,
            line_limit: LINE_LIMIT,
        }
    }

    pub(crate) const fn jsonrpc(name: &'static str, exited_early: &'static str) -> Self {
        Self {
            jsonrpc: true,
            ..Self::jsonrpc_lite(name, exited_early)
        }
    }

    #[cfg(test)]
    pub(crate) const fn within(self, timeout: Duration, line_limit: usize) -> Self {
        Self {
            timeout,
            line_limit,
            ..self
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NotStarted {
    Missing,
    Failed(String),
}

#[derive(Debug)]
pub(crate) struct Failure {
    kind: FailureKind,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureKind {
    Closed,
    Timeout,
    InvalidOutput,
    Protocol,
    Rejected,
    Other,
}

impl Failure {
    pub(crate) fn rejected(message: impl Into<String>) -> Self {
        Self::new(FailureKind::Rejected, message)
    }

    fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

pub(crate) struct Request<'a> {
    id: u64,
    method: &'a str,
    params: Value,
    when_missing: Option<&'a str>,
}

impl<'a> Request<'a> {
    pub(crate) fn new(id: u64, method: &'a str) -> Self {
        Self {
            id,
            method,
            params: json!({}),
            when_missing: None,
        }
    }

    pub(crate) fn params(self, params: Value) -> Self {
        Self { params, ..self }
    }

    pub(crate) fn when_missing(self, guidance: &'a str) -> Self {
        Self {
            when_missing: Some(guidance),
            ..self
        }
    }
}

pub(crate) struct Session {
    peer: Peer,
    transport: Box<dyn Transport>,
    initialized: bool,
}

impl Session {
    pub(crate) fn start(peer: Peer, mut command: Command) -> Result<Self, NotStarted> {
        Ok(Self::over(peer, Process::spawn(peer, &mut command)?))
    }

    fn over(peer: Peer, transport: impl Transport + 'static) -> Self {
        Self {
            peer,
            transport: Box::new(transport),
            initialized: false,
        }
    }

    pub(crate) fn handshake<R: DeserializeOwned>(
        &mut self,
        capabilities: Option<Value>,
        accept: impl FnOnce(&R) -> Result<(), String>,
    ) -> Result<R, Failure> {
        let mut params = json!({"clientInfo": {
            "name": "on_n_off",
            "title": "on-n-off",
            "version": env!("CARGO_PKG_VERSION"),
        }});
        if let Some(capabilities) = capabilities {
            params["capabilities"] = capabilities;
        }
        let answer = self.request(Request::new(1, "initialize").params(params))?;
        self.initialized = true;
        accept(&answer).map_err(Failure::rejected)?;
        self.send(None, "initialized", json!({}))?;
        Ok(answer)
    }

    pub(crate) fn request<R: DeserializeOwned>(&mut self, request: Request) -> Result<R, Failure> {
        self.send(Some(request.id), request.method, request.params)?;
        loop {
            let message = self.transport.receive()?;
            if message.get("id").and_then(Value::as_u64) == Some(request.id) {
                return answer(self.peer, &message, request.method, request.when_missing);
            }
        }
    }

    pub(crate) fn run<T>(
        mut self,
        talk: impl FnOnce(&mut Self) -> Result<T, Failure>,
    ) -> Result<T, String> {
        let talked = talk(&mut self);
        let exit = self.transport.finish();
        talked.map_err(|failure| explained(self.peer, self.initialized, failure, exit))
    }

    fn send(&mut self, id: Option<u64>, method: &str, params: Value) -> Result<(), Failure> {
        let mut message = Map::new();
        if self.peer.jsonrpc {
            message.insert("jsonrpc".to_string(), json!("2.0"));
        }
        if let Some(id) = id {
            message.insert("id".to_string(), json!(id));
        }
        message.insert("method".to_string(), json!(method));
        message.insert("params".to_string(), params);
        self.transport.send(&Value::Object(message))
    }
}

fn answer<R: DeserializeOwned>(
    peer: Peer,
    message: &Value,
    method: &str,
    when_missing: Option<&str>,
) -> Result<R, Failure> {
    let name = peer.name;
    if let Some(result) = message.get("result") {
        return serde_json::from_value(result.clone()).map_err(|error| {
            Failure::new(
                FailureKind::Protocol,
                format!("{name} `{method}` returned a malformed response: {error}"),
            )
        });
    }
    let error = message.get("error");
    let code = error
        .and_then(|error| error.get("code"))
        .and_then(Value::as_i64);
    let detail = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("unknown protocol error");
    let guidance = when_missing
        .filter(|_| code == Some(METHOD_NOT_FOUND))
        .map_or_else(String::new, |guidance| format!(" {guidance}"));
    let code = code.map_or_else(String::new, |code| format!(" (code {code})"));
    Err(Failure::new(
        FailureKind::Protocol,
        format!("{name} `{method}` failed{code}: {detail}.{guidance}"),
    ))
}

fn explained(peer: Peer, initialized: bool, failure: Failure, exit: Option<ExitStatus>) -> String {
    if !initialized
        && failure.kind == FailureKind::Closed
        && exit.is_some_and(|status| !status.success())
    {
        return peer.exited_early.to_string();
    }
    failure.message
}

trait Transport {
    fn send(&mut self, message: &Value) -> Result<(), Failure>;
    fn receive(&mut self) -> Result<Value, Failure>;
    fn finish(&mut self) -> Option<ExitStatus>;
}

struct Process {
    name: &'static str,
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Option<Receiver<Result<Value, Failure>>>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
    deadline: Instant,
}

impl Process {
    fn spawn(peer: Peer, command: &mut Command) -> Result<Self, NotStarted> {
        let name = peer.name;
        let line_limit = peer.line_limit;
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    NotStarted::Missing
                } else {
                    NotStarted::Failed(format!("Could not start {name}: {error}"))
                }
            })?;
        let unavailable =
            |pipe: &str| NotStarted::Failed(format!("{name} {pipe} was not available."));
        let stdin = child.stdin.take().ok_or_else(|| unavailable("stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| unavailable("stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| unavailable("stderr"))?;
        let (send, messages) = mpsc::sync_channel(MESSAGE_QUEUE_LIMIT);
        let stdout_thread = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = Vec::new();
            loop {
                let message = match read_bounded_line(&mut reader, &mut line, line_limit) {
                    Ok(None) => break,
                    Ok(Some(true)) => Err(Failure::new(
                        FailureKind::InvalidOutput,
                        format!("{name} output line exceeded {line_limit} bytes."),
                    )),
                    Ok(Some(false)) => serde_json::from_slice::<Value>(&line).map_err(|error| {
                        Failure::new(
                            FailureKind::InvalidOutput,
                            format!("{name} returned invalid JSON: {error}"),
                        )
                    }),
                    Err(error) => Err(Failure::new(
                        FailureKind::Other,
                        format!("Could not read {name} output: {error}"),
                    )),
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
            name,
            child,
            stdin: Some(stdin),
            messages: Some(messages),
            stdout_thread: Some(stdout_thread),
            stderr_thread: Some(stderr_thread),
            deadline: Instant::now() + peer.timeout,
        })
    }
}

impl Transport for Process {
    fn send(&mut self, message: &Value) -> Result<(), Failure> {
        let name = self.name;
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| Failure::new(FailureKind::Closed, format!("{name} input is closed.")))?;
        serde_json::to_writer(&mut *stdin, message).map_err(|error| {
            Failure::new(
                FailureKind::Other,
                format!("Could not encode {name} request: {error}"),
            )
        })?;
        stdin
            .write_all(b"\n")
            .and_then(|()| stdin.flush())
            .map_err(|error| {
                Failure::new(
                    if matches!(
                        error.kind(),
                        io::ErrorKind::BrokenPipe
                            | io::ErrorKind::ConnectionAborted
                            | io::ErrorKind::ConnectionReset
                            | io::ErrorKind::NotConnected
                    ) {
                        FailureKind::Closed
                    } else {
                        FailureKind::Other
                    },
                    format!("Could not write to {name}: {error}"),
                )
            })
    }

    fn receive(&mut self) -> Result<Value, Failure> {
        let name = self.name;
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Failure::new(
                FailureKind::Timeout,
                format!("{name} timed out."),
            ));
        }
        self.messages
            .as_ref()
            .ok_or_else(|| Failure::new(FailureKind::Closed, format!("{name} output is closed.")))?
            .recv_timeout(remaining)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    Failure::new(FailureKind::Timeout, format!("{name} timed out."))
                }
                mpsc::RecvTimeoutError::Disconnected => Failure::new(
                    FailureKind::Closed,
                    format!("{name} closed before returning limits."),
                ),
            })?
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
            let status = self.child.wait().ok();
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

impl Drop for Process {
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
mod script;
#[cfg(test)]
pub(crate) use script::{Exchange, Script};

#[cfg(test)]
mod tests;
