use std::process::{Command, Stdio};
use std::time::Duration;

use crate::cli_locate::{binary_on_path, cli_search_path_value, resolve_cli_binary};
use crate::dto::{AdapterError, ErrorKind};
use crate::install_source::InstallSource;
use crate::process::{wait_with_deadline, CommandOutcome};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(45);
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(180);
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
        self.run_timed(args, self.timeout)
    }

    pub fn run_args(&self, args: &[String]) -> Result<String, AdapterError> {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(&refs)
    }

    pub fn run_args_timed(
        &self,
        args: &[String],
        timeout: Duration,
    ) -> Result<String, AdapterError> {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run_timed(&refs, timeout)
    }

    /// Build a provider CLI command with the same binary resolution and GUI-safe `PATH` as the
    /// ordinary bounded runner. Interactive protocols can configure stdio and lifecycle handling
    /// before spawning it.
    pub(crate) fn command(&self) -> Command {
        let mut command = Command::new(
            resolve_cli_binary(&self.binary)
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.binary.clone()),
        );
        // Node-based CLIs are `#!/usr/bin/env node` shims: the child needs the same PATH the
        // CLI was found on, not the (possibly minimal) PATH a GUI app inherited.
        if let Some(path) = cli_search_path_value() {
            command.env("PATH", path);
        }
        command
    }

    fn run_timed(&self, args: &[&str], timeout: Duration) -> Result<String, AdapterError> {
        let child = self
            .command()
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| spawn_error(&self.binary, error))?;
        match wait_with_deadline(child, timeout)
            .map_err(|error| AdapterError::message(error.to_string()))?
        {
            CommandOutcome::Exited {
                success: true,
                stdout,
                ..
            } => Ok(stdout),
            CommandOutcome::Exited { stdout, stderr, .. } => {
                Err(AdapterError::message(trim_cli(&stderr, &stdout)))
            }
            CommandOutcome::TimedOut => {
                Err(AdapterError::message(format!("{} timed out.", self.binary)))
            }
        }
    }
}

pub fn run_npx_skills(source: &InstallSource, agent: &str) -> Result<String, AdapterError> {
    if !source.is_npx_skills() {
        return Err(AdapterError::message("not an npx skills install"));
    }
    if !binary_on_path("npx") {
        return Err(AdapterError::message(
            "npx not found. Skill install needs Node.js.",
        ));
    }
    AgentCli::new("npx").run_args_timed(&source.npx_skills_argv(agent), INSTALL_TIMEOUT)
}

fn spawn_error(binary: &str, error: std::io::Error) -> AdapterError {
    if error.kind() == std::io::ErrorKind::NotFound {
        return AdapterError {
            kind: ErrorKind::CliMissing,
            message: format!("{binary} CLI not found."),
            path: None,
        };
    }
    if cfg!(unix) && error.kind() == std::io::ErrorKind::PermissionDenied {
        return AdapterError::message(format!(
            "{binary} is not executable (permission denied). Run `chmod +x` on it or point Binary at the real launcher."
        ));
    }
    if error.raw_os_error() == Some(193) {
        return AdapterError::message(format!(
            "{binary} is not a Windows program (os error 193). Point Binary at the .cmd or .exe launcher — nvm/npm often leave an extensionless shim that cmd cannot run."
        ));
    }
    AdapterError::message(error.to_string())
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
    use crate::cli_stub::CliStub;
    use std::path::PathBuf;
    use std::time::Instant;

    fn stub_dir() -> std::path::PathBuf {
        crate::paths::scratch_dir("on-n-off-cli")
    }

    #[test]
    fn success_returns_stdout() {
        let out = CliStub::new("ok")
            .stdout("{\"ok\":true}")
            .cli(&stub_dir())
            .run(&["plugin", "list"])
            .expect("ok");
        assert!(out.contains("ok"));
    }

    #[test]
    fn non_zero_returns_stderr() {
        let err = CliStub::new("fail")
            .stderr("boom")
            .exit(2)
            .cli(&stub_dir())
            .run(&["plugin", "enable", "x"])
            .expect_err("fail");
        assert_eq!(err.kind, ErrorKind::Message);
        assert!(err.message.contains("boom"));
    }

    #[test]
    fn timeout_kills_the_process() {
        let cli = CliStub::new("slow").sleep(7).cli(&stub_dir());
        let started = Instant::now();
        let err = cli
            .with_timeout(Duration::from_millis(200))
            .run(&["plugin", "enable", "x"])
            .expect_err("timeout");
        assert!(err.message.to_lowercase().contains("timed out"));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "timeout returned after {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn drains_chatty_stdout_and_stderr_while_the_process_runs() {
        let out = CliStub::new("chatty")
            .chatty(5000)
            .stdout("complete")
            .cli(&stub_dir())
            .with_timeout(Duration::from_secs(5))
            .run(&[])
            .expect("chatty child must not block on full output pipes");
        assert!(out.contains("complete"));
    }

    #[cfg(unix)]
    #[test]
    fn non_executable_launcher_gets_a_permission_hint() {
        // Not an agent name: agent-named binaries fall through to the user's settings overrides.
        let path = stub_dir().join("noexec-tool");
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        let err = AgentCli::new(path.to_string_lossy().as_ref())
            .run(&["--version"])
            .expect_err("permission denied");
        assert!(
            err.message.contains("not executable") && err.message.contains("noexec-tool"),
            "unexpected message: {}",
            err.message
        );
    }

    #[test]
    fn children_get_the_cli_search_path() {
        let printed = CliStub::new("path-echo")
            .print_env("PATH")
            .cli(&stub_dir())
            .run(&[])
            .expect("path-echo");
        let printed_dirs: Vec<PathBuf> = std::env::split_paths(printed.trim()).collect();
        let search_path = crate::cli_locate::cli_search_path();
        for dir in search_path {
            assert!(
                printed_dirs.contains(dir),
                "child PATH lacks {dir:?}: {printed}"
            );
        }
        // The well-known tier (not just the process PATH) must reach the child. Compared by
        // suffix because other tests re-point ON_N_OFF_HOME while this process runs.
        assert!(
            search_path
                .iter()
                .any(|dir| dir.ends_with(std::path::Path::new(".local").join("bin"))),
            "well-known dirs missing from the search path: {search_path:?}"
        );
    }

    #[test]
    fn missing_binary_is_cli_missing() {
        let err = AgentCli::new("on-n-off-no-such-cli.exe")
            .run(&["plugin", "list"])
            .expect_err("missing");
        assert_eq!(err.kind, ErrorKind::CliMissing);
    }
}
