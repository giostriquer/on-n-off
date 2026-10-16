use super::host::{self, Inbound, Link, RailData, Transport};
use super::model::NotchSnapshot;
use super::protocol::{decode_event, encode, Action, MAX_MESSAGE};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub fn executable() -> Result<PathBuf, String> {
    let path = std::env::current_exe().map_err(|e| e.to_string())?;
    let folder = path.parent().ok_or("Cannot locate the native notch.")?;
    let folder = if cfg!(test) && folder.file_name().is_some_and(|n| n == "deps") {
        folder.parent().ok_or("Cannot locate the native notch.")?
    } else {
        folder
    };
    let bundled = folder.join("../Helpers/on-n-off-notch.app/Contents/MacOS/on-n-off-notch");
    let binary = if folder.file_name().is_some_and(|name| name == "MacOS") {
        bundled
    } else {
        folder.join("on-n-off-notch")
    };
    if !binary.is_file() {
        return Err(
            "The native notch is missing from this app. Rebuild or reinstall on-n-off.".into(),
        );
    }
    Ok(binary)
}

const ACK_DEADLINE: Duration = Duration::from_secs(10);
const STEADY: Duration = Duration::from_secs(30);
const MAX_RETRY_SECONDS: u64 = 30;

struct Connection {
    child: Arc<Mutex<Child>>,
    writer: Option<SyncSender<Vec<u8>>>,
    events: Receiver<Result<Action, String>>,
}

#[derive(Default)]
struct LifetimeState {
    stopped: bool,
    child: Option<Arc<Mutex<Child>>>,
}

#[derive(Default)]
pub struct Lifetime(Mutex<LifetimeState>);

impl Lifetime {
    fn connect(&self) -> Result<Connection, String> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped {
            return Err("Native notch is shutting down.".into());
        }
        let connection = Connection::start()?;
        state.child = Some(connection.child.clone());
        Ok(connection)
    }
}

impl host::Lifetime for Lifetime {
    fn stopped(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).stopped
    }

    fn shutdown(&self) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.stopped = true;
        if let Some(child) = state.child.take() {
            stop_child(&child);
        }
    }
}

pub struct Pipe {
    lifetime: Arc<Lifetime>,
    connection: Option<Connection>,
    connected_at: Instant,
    sequence: u64,
    awaiting_ack: Option<(u64, Instant)>,
    next_start: Instant,
    retry_delay: u64,
}

impl Pipe {
    pub fn new() -> (Self, Arc<Lifetime>) {
        let lifetime = Arc::new(Lifetime::default());
        let pipe = Self {
            lifetime: lifetime.clone(),
            connection: None,
            connected_at: Instant::now(),
            sequence: 0,
            awaiting_ack: None,
            next_start: Instant::now(),
            retry_delay: 1,
        };
        (pipe, lifetime)
    }

    fn back_off(&mut self) {
        self.next_start = Instant::now() + Duration::from_secs(self.retry_delay);
        self.retry_delay = (self.retry_delay * 2).min(MAX_RETRY_SECONDS);
    }

    fn fail(&mut self, error: String) -> String {
        self.connection = None;
        self.awaiting_ack = None;
        self.back_off();
        error
    }
}

impl Transport for Pipe {
    fn connect(&mut self) -> Result<Link, String> {
        if self.connection.is_some() {
            return Ok(Link::Open);
        }
        if Instant::now() < self.next_start {
            return Ok(Link::Closed);
        }
        match self.lifetime.connect() {
            Ok(connection) => {
                self.connection = Some(connection);
                self.connected_at = Instant::now();
                Ok(Link::Opened)
            }
            Err(error) => {
                self.back_off();
                Err(error)
            }
        }
    }

    fn receive(&mut self, inbound: &mut Vec<Inbound>) -> Result<(), String> {
        let Some(connection) = &self.connection else {
            return Ok(());
        };
        let failure = loop {
            let action = match connection.events.try_recv() {
                Ok(Ok(Action::Ready)) => continue,
                Ok(Ok(Action::Ack { sequence })) => {
                    if self
                        .awaiting_ack
                        .is_some_and(|(awaited, _)| awaited == sequence)
                    {
                        self.awaiting_ack = None;
                        if self.connected_at.elapsed() >= STEADY {
                            self.retry_delay = 1;
                        }
                        inbound.push(Inbound::Confirmed);
                    }
                    continue;
                }
                Ok(Ok(Action::ScreensChanged)) => host::Action::ScreensChanged,
                Ok(Ok(Action::Refresh)) => host::Action::Refresh,
                Ok(Ok(Action::OpenLimits)) => host::Action::OpenLimits,
                Ok(Ok(Action::OpenPullRequests)) => host::Action::OpenPullRequests,
                Ok(Ok(Action::SetShow { show })) => host::Action::SetShow(show),
                Ok(Err(error)) => break Some(error),
                Err(mpsc::TryRecvError::Empty) => break None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    break Some("Native notch stopped unexpectedly. Retrying…".to_string());
                }
            };
            inbound.push(Inbound::Action(action));
        };
        let unanswered = self
            .awaiting_ack
            .is_some_and(|(_, sent)| sent.elapsed() > ACK_DEADLINE);
        let failure = if unanswered {
            Some("Native notch stopped responding. Retrying…".to_string())
        } else {
            failure
        };
        failure.map_or(Ok(()), |error| Err(self.fail(error)))
    }

    fn ready(&self) -> bool {
        self.awaiting_ack.is_none()
    }

    fn send(&mut self, snapshot: &NotchSnapshot, frame: RailData) -> Result<(), String> {
        let Some(connection) = &self.connection else {
            return Ok(());
        };
        self.sequence += 1;
        match encode(self.sequence, snapshot, &frame).and_then(|message| connection.send(message)) {
            Ok(()) => {
                self.awaiting_ack = Some((self.sequence, Instant::now()));
                Ok(())
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    fn close(&mut self) {
        self.connection = None;
        self.awaiting_ack = None;
        self.retry_delay = 1;
    }

    fn settings_changed(&mut self) {
        self.next_start = Instant::now();
    }
}

fn stop_child(child: &Mutex<Child>) {
    let mut child = child.lock().unwrap_or_else(|e| e.into_inner());
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let _ = child.kill();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Err(_) => break,
            Ok(None) if Instant::now() >= deadline => break,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    }
    eprintln!("Native notch child cleanup did not complete before its deadline.");
}

impl Connection {
    fn start() -> Result<Self, String> {
        let mut child = Command::new(executable()?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Cannot start native notch: {e}"))?;
        let mut input = child.stdin.take().ok_or("Missing native notch input.")?;
        let output = child.stdout.take().ok_or("Missing native notch output.")?;
        let mut errors = child
            .stderr
            .take()
            .ok_or("Missing native notch diagnostics.")?;
        let (writer, writes) = mpsc::sync_channel::<Vec<u8>>(1);
        thread::spawn(move || {
            while let Ok(bytes) = writes.recv() {
                if input.write_all(&bytes).is_err() {
                    break;
                }
            }
        });
        thread::spawn(move || {
            let _ = std::io::copy(&mut errors, &mut std::io::sink());
        });
        let (sender, events) = mpsc::sync_channel(32);
        thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut line = Vec::new();
                let result = reader
                    .by_ref()
                    .take((MAX_MESSAGE + 1) as u64)
                    .read_until(b'\n', &mut line);
                let result = match result {
                    Ok(0) => break,
                    Ok(_) if line.last() == Some(&b'\n') => decode_event(&line),
                    _ => Err("Native notch connection failed.".into()),
                };
                let failed = result.is_err();
                if sender.try_send(result).is_err() || failed {
                    break;
                }
            }
        });
        Ok(Self {
            child: Arc::new(Mutex::new(child)),
            writer: Some(writer),
            events,
        })
    }
    fn send(&self, mut message: Vec<u8>) -> Result<(), String> {
        if message.len() > MAX_MESSAGE {
            return Err("Native notch snapshot is too large.".into());
        }
        message.push(b'\n');
        self.writer
            .as_ref()
            .ok_or("Native notch stopped.")?
            .try_send(message)
            .map_err(|_| "Native notch is not reading updates.".into())
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.writer.take();
        stop_child(&self.child);
    }
}

#[cfg(test)]
mod tests;
