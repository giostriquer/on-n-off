use std::io::{BufRead, BufReader, Read};
use std::process::Child;
use std::sync::mpsc::{self, Sender};
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

pub(crate) fn wait_with_deadline(
    mut child: Child,
    timeout: Duration,
) -> std::io::Result<CommandOutcome> {
    let stdout = child.stdout.take().map(read_pipe);
    let stderr = child.stderr.take().map(read_pipe);
    wait(child, stdout, stderr, timeout, || false, || {})
}

pub(crate) fn wait_with_cancellation(
    mut child: Child,
    timeout: Duration,
    canceled: impl Fn() -> bool,
    mut on_line: impl FnMut(&str),
) -> std::io::Result<CommandOutcome> {
    let (sender, lines) = mpsc::channel();
    let stdout = child.stdout.take().map(|pipe| read_lines(pipe, sender));
    let stderr = child.stderr.take().map(read_pipe);
    let mut deliver = || lines.try_iter().for_each(|line| on_line(&line));
    let outcome = wait(child, stdout, stderr, timeout, canceled, &mut deliver)?;
    if matches!(outcome, CommandOutcome::Exited { .. }) {
        deliver();
    }
    Ok(outcome)
}

fn wait(
    mut child: Child,
    stdout: Option<JoinHandle<String>>,
    stderr: Option<JoinHandle<String>>,
    timeout: Duration,
    canceled: impl Fn() -> bool,
    mut between_polls: impl FnMut(),
) -> std::io::Result<CommandOutcome> {
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
            Ok(None) if started.elapsed() >= timeout || canceled() => {
                let _ = child.kill();
                let _ = child.wait();
                drop(stdout);
                drop(stderr);
                return Ok(CommandOutcome::TimedOut);
            }
            Ok(None) => {
                between_polls();
                std::thread::sleep(Duration::from_millis(20));
            }
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

fn read_lines<R: Read + Send + 'static>(pipe: R, lines: Sender<String>) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut pipe = BufReader::new(pipe);
        let mut output = Vec::new();
        let mut line = Vec::new();
        while pipe.read_until(b'\n', &mut line).is_ok_and(|read| read > 0) {
            let text = String::from_utf8_lossy(&line);
            let _ = lines.send(text.trim_end_matches(['\n', '\r']).to_owned());
            output.append(&mut line);
        }
        String::from_utf8(output).unwrap_or_default()
    })
}

fn join_pipe(reader: Option<JoinHandle<String>>) -> String {
    reader
        .and_then(|thread| thread.join().ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
