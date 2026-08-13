use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::dto::{AdapterError, ErrorKind};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(45);
const STDERR_LIMIT: usize = 400;

pub struct AgentCli {
    binary: String,
    timeout: Duration,
}

impl AgentCli {
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self { timeout, ..self }
    }

    pub fn run(&self, args: &[&str]) -> Result<String, AdapterError> {
        let mut child = Command::new(&self.binary)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    AdapterError {
                        kind: ErrorKind::CliMissing,
                        message: format!("{} CLI not found.", self.binary),
                        path: None,
                    }
                } else {
                    AdapterError::message(error.to_string())
                }
            })?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut stdout = String::new();
                    let mut stderr = String::new();
                    if let Some(mut out) = child.stdout.take() {
                        let _ = out.read_to_string(&mut stdout);
                    }
                    if let Some(mut err) = child.stderr.take() {
                        let _ = err.read_to_string(&mut stderr);
                    }
                    if status.success() {
                        return Ok(stdout);
                    }
                    return Err(AdapterError::message(trim_cli(&stderr, &stdout)));
                }
                Ok(None) if started.elapsed() >= self.timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AdapterError::message(format!(
                        "{} timed out.",
                        self.binary
                    )));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(error) => return Err(AdapterError::message(error.to_string())),
            }
        }
    }
}

fn trim_cli(stderr: &str, stdout: &str) -> String {
    let text = if !stderr.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };
    if text.is_empty() {
        return "CLI failed.".to_string();
    }
    if text.chars().count() > STDERR_LIMIT {
        let clipped: String = text.chars().take(STDERR_LIMIT).collect();
        return format!("{clipped}…");
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn stub(name: &str, body: &str) -> std::path::PathBuf {
        let dir = crate::paths::scratch_dir("on-n-off-cli");
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn success_returns_stdout() {
        let bin = stub(
            "ok.cmd",
            "@echo off\r\necho {\"ok\":true}\r\nexit /b 0\r\n",
        );
        let out = AgentCli::new(bin.to_string_lossy().as_ref())
            .run(&["plugin", "list"])
            .expect("ok");
        assert!(out.contains("ok"));
    }

    #[test]
    fn non_zero_returns_stderr() {
        let bin = stub(
            "fail.cmd",
            "@echo off\r\necho boom 1>&2\r\nexit /b 2\r\n",
        );
        let err = AgentCli::new(bin.to_string_lossy().as_ref())
            .run(&["plugin", "enable", "x"])
            .expect_err("fail");
        assert_eq!(err.kind, ErrorKind::Message);
        assert!(err.message.contains("boom"));
    }

    #[test]
    fn timeout_kills_the_process() {
        let bin = stub(
            "slow.cmd",
            "@echo off\r\nping -n 8 127.0.0.1 >nul\r\nexit /b 0\r\n",
        );
        let err = AgentCli::new(bin.to_string_lossy().as_ref())
            .with_timeout(Duration::from_millis(200))
            .run(&["plugin", "enable", "x"])
            .expect_err("timeout");
        assert!(err.message.to_lowercase().contains("timed out"));
    }

    #[test]
    fn missing_binary_is_cli_missing() {
        let err = AgentCli::new("on-n-off-no-such-cli.exe")
            .run(&["plugin", "list"])
            .expect_err("missing");
        assert_eq!(err.kind, ErrorKind::CliMissing);
    }
}
