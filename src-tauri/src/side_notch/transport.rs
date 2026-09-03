use super::protocol::{decode_event, Action, MAX_MESSAGE};
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
    // Cargo test executables live one level below the development sidecar.
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

pub struct Connection {
    child: Arc<Mutex<Child>>,
    writer: Option<SyncSender<Vec<u8>>>,
    pub events: Receiver<Result<Action, String>>,
}

#[derive(Default)]
struct LifetimeState {
    stopped: bool,
    child: Option<Arc<Mutex<Child>>>,
}

/// The exit callback owns the child independently of the background supervisor.
#[derive(Default)]
pub struct Lifetime(Mutex<LifetimeState>);

impl Lifetime {
    pub fn stopped(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).stopped
    }

    pub fn connect(&self) -> Result<Connection, String> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped {
            return Err("Native notch is shutting down.".into());
        }
        let connection = Connection::start()?;
        state.child = Some(connection.child.clone());
        Ok(connection)
    }

    pub fn shutdown(&self) {
        // Serialize spawning with shutdown: no child can appear after exit's
        // cleanup. Never wait for the supervisor's filesystem/network work.
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.stopped = true;
        if let Some(child) = state.child.take() {
            stop_child(&child);
        }
    }
}

fn stop_child(child: &Mutex<Child>) {
    let mut child = child.lock().unwrap_or_else(|e| e.into_inner());
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    // SIGKILL also releases a writer blocked on a hung helper's stdin pipe.
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
        // Do not retain or log provider-facing payloads from a child process.
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
    pub fn send(&self, mut message: Vec<u8>) -> Result<(), String> {
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
