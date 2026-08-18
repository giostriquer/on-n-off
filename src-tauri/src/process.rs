//! Child-process plumbing shared by the agent CLI runner and the login-shell probe.

use std::io::Read;
use std::process::Child;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(crate) enum CommandOutcome {
    Exited {
        success: bool,
        stdout: String,
        stderr: String,
    },
    TimedOut,
}

/// Drain a child's pipes while it runs and enforce `timeout` as a hard deadline.
///
/// The child must have been spawned with piped stdout/stderr. On timeout the child is
/// killed and its drainers are detached, since a spawned descendant may still own the
/// inherited pipe handles.
pub(crate) fn wait_with_deadline(
    mut child: Child,
    timeout: Duration,
) -> std::io::Result<CommandOutcome> {
    let stdout = child.stdout.take().map(read_pipe);
    let stderr = child.stderr.take().map(read_pipe);
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(CommandOutcome::Exited {
                    success: status.success(),
                    stdout: join_pipe(stdout),
                    stderr: join_pipe(stderr),
                });
            }
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                drop(stdout);
                drop(stderr);
                return Ok(CommandOutcome::TimedOut);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                drop(stdout);
                drop(stderr);
                return Err(error);
            }
        }
    }
}

fn read_pipe<R: Read + Send + 'static>(mut pipe: R) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut output = String::new();
        let _ = pipe.read_to_string(&mut output);
        output
    })
}

fn join_pipe(reader: Option<JoinHandle<String>>) -> String {
    reader
        .and_then(|thread| thread.join().ok())
        .unwrap_or_default()
}
