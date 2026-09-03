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
mod tests;
