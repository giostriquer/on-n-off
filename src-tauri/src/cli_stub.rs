//! Test-only builder for fake agent CLIs.
//!
//! Adapter tests need a stand-in for `claude` / `codex` / `agy` that records its argv,
//! prints canned output, and exits with a chosen code. The stand-in has to be a real
//! program the OS can spawn: a `.cmd` batch file on Windows and an executable `sh`
//! script elsewhere. Callers describe the behavior once and this module writes the
//! platform-appropriate launcher.

use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::AgentCli;

const CHATTY_PAYLOAD: &str = "abcdefghijklmnopqrstuvwxyz0123456789";

#[derive(Debug, Default)]
pub struct CliStub {
    name: String,
    copy: Option<(String, String)>,
    args_log: Option<(String, bool)>,
    chatty_lines: usize,
    print_env: Option<String>,
    stdout: Option<String>,
    stderr: Option<String>,
    sleep_secs: u32,
    exit: i32,
}

impl CliStub {
    /// `name` is the launcher name without an extension, e.g. `claude`.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Self::default()
        }
    }

    /// Copy `from` to `to` (both relative to the stub's directory) before anything else.
    pub fn copy(mut self, from: &str, to: &str) -> Self {
        self.copy = Some((from.to_string(), to.to_string()));
        self
    }

    /// Write the received argv to `file` (relative to the stub's directory).
    pub fn log_args(mut self, file: &str, append: bool) -> Self {
        self.args_log = Some((file.to_string(), append));
        self
    }

    /// Emit `lines` long lines on both stdout and stderr to fill the pipes.
    pub fn chatty(mut self, lines: usize) -> Self {
        self.chatty_lines = lines;
        self
    }

    /// Print the value of environment variable `name` on stdout.
    pub fn print_env(mut self, name: &str) -> Self {
        self.print_env = Some(name.to_string());
        self
    }

    pub fn stdout(mut self, text: &str) -> Self {
        self.stdout = Some(text.to_string());
        self
    }

    pub fn stderr(mut self, text: &str) -> Self {
        self.stderr = Some(text.to_string());
        self
    }

    pub fn sleep(mut self, secs: u32) -> Self {
        self.sleep_secs = secs;
        self
    }

    pub fn exit(mut self, code: i32) -> Self {
        self.exit = code;
        self
    }

    /// Write the launcher into `dir` and return its path.
    pub fn write(&self, dir: &Path) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(launcher_file_name(&self.name));
        if cfg!(windows) {
            fs::write(&path, self.batch_body()).unwrap();
        } else {
            fs::write(&path, self.sh_body()).unwrap();
            mark_executable(&path);
        }
        path
    }

    /// Write the launcher into `dir` and wrap it in an [`AgentCli`].
    pub fn cli(&self, dir: &Path) -> AgentCli {
        AgentCli::new(self.write(dir).to_string_lossy().as_ref())
    }

    fn batch_body(&self) -> String {
        let mut lines = vec!["@echo off".to_string()];
        if let Some((from, to)) = &self.copy {
            lines.push(format!(
                "copy /Y \"%~dp0{}\" \"%~dp0{}\" >nul",
                windows_relative(from),
                windows_relative(to)
            ));
        }
        if let Some((file, append)) = &self.args_log {
            let redirect = if *append { ">>" } else { ">" };
            lines.push(format!(
                "echo %* {redirect} \"%~dp0{}\"",
                windows_relative(file)
            ));
        }
        if self.chatty_lines > 0 {
            lines.push(format!(
                "for /L %%i in (1,1,{}) do @(echo stdout-%%i-{CHATTY_PAYLOAD}& echo stderr-%%i-{CHATTY_PAYLOAD} 1>&2)",
                self.chatty_lines
            ));
        }
        if let Some(name) = &self.print_env {
            lines.push(format!("echo %{name}%"));
        }
        if let Some(text) = &self.stdout {
            lines.push(format!("echo {text}"));
        }
        if let Some(text) = &self.stderr {
            lines.push(format!("echo {text} 1>&2"));
        }
        if self.sleep_secs > 0 {
            lines.push(format!("ping -n {} 127.0.0.1 >nul", self.sleep_secs + 1));
        }
        lines.push(format!("exit /b {}", self.exit));
        format!("{}\r\n", lines.join("\r\n"))
    }

    fn sh_body(&self) -> String {
        let mut lines = vec![
            "#!/bin/sh".to_string(),
            "here=\"$(cd \"$(dirname \"$0\")\" && pwd)\"".to_string(),
        ];
        if let Some((from, to)) = &self.copy {
            lines.push(format!("cp \"$here/{from}\" \"$here/{to}\""));
        }
        if let Some((file, append)) = &self.args_log {
            let redirect = if *append { ">>" } else { ">" };
            lines.push(format!("printf '%s\\n' \"$*\" {redirect} \"$here/{file}\""));
        }
        if self.chatty_lines > 0 {
            lines.push(format!(
                "i=1; while [ \"$i\" -le {} ]; do echo \"stdout-$i-{CHATTY_PAYLOAD}\"; echo \"stderr-$i-{CHATTY_PAYLOAD}\" >&2; i=$((i + 1)); done",
                self.chatty_lines
            ));
        }
        if let Some(name) = &self.print_env {
            lines.push(format!("printf '%s\\n' \"${name}\""));
        }
        if let Some(text) = &self.stdout {
            lines.push(format!("printf '%s\\n' '{text}'"));
        }
        if let Some(text) = &self.stderr {
            lines.push(format!("printf '%s\\n' '{text}' >&2"));
        }
        if self.sleep_secs > 0 {
            lines.push(format!("sleep {}", self.sleep_secs));
        }
        lines.push(format!("exit {}", self.exit));
        format!("{}\n", lines.join("\n"))
    }
}

fn launcher_file_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_string()
    }
}

fn windows_relative(path: &str) -> String {
    path.replace('/', "\\")
}

#[cfg(unix)]
fn mark_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

#[cfg(not(unix))]
fn mark_executable(_path: &Path) {}
