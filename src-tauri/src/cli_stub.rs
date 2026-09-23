//! Test-only builder for fake agent CLIs.
//!
//! Adapter tests need a stand-in for `claude` / `codex` / `agy` that records its argv,
//! prints canned output, and exits with a chosen code. The stand-in has to be a real
//! program the OS can spawn: a `.cmd` batch file on Windows and an executable `sh`
//! script elsewhere. Callers describe the behavior once and this module writes the
//! platform-appropriate launcher.
//!
//! Launchers are shared by content. Endpoint security on a developer Mac can hold a newly written
//! script for seconds the first time it runs, and it pays that once per file, not per path: a
//! hard link to a file that has already run starts at once. A test that gives its stub a few
//! seconds to answer fails on that first start alone. So each distinct launcher body is written
//! once, under the temp dir and named by a hash of its content, run there once with
//! [`WARM_UP_VAR`] set (every body exits straight away on it), and then hard-linked into the
//! directory the test asked for. The first start is paid while the test is still setting up, and
//! because the shared files outlive the run, later runs skip it.
//!
//! A link keeps everything else as it was. `$0` and `%~dp0` still name the test's own directory,
//! so argument logs and copies stay private to it. On Unix the shared file is read-only, so no
//! test can change another test's stub by writing through its link, and on every platform a stub
//! written again under one name replaces its link or fails, never writing through it. Where no
//! link can be made, the launcher is a private copy, as before.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use crate::cli::AgentCli;

/// How long a test waits for a stub, or for a script it has just written, to answer.
///
/// Generous on purpose. A first start can take seconds on a loaded machine (see above), and how
/// fast a launcher starts is never what such a test checks. A test about giving up in time keeps
/// its own short deadline and bounds how long giving up took.
pub const ANSWER_DEADLINE: Duration = Duration::from_secs(60);

const CHATTY_PAYLOAD: &str = "abcdefghijklmnopqrstuvwxyz0123456789";

/// Set only for the one run that warms a shared launcher, which makes the launcher exit at once.
const WARM_UP_VAR: &str = "ON_N_OFF_STUB_WARMUP";

/// A test's own launcher: it stays writable, as a written file always was.
const PRIVATE_MODE: u32 = 0o755;
/// A launcher every test with the same body links to.
const SHARED_MODE: u32 = 0o555;

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
        let body = self.body();
        // An earlier stub of this name may be a link to a shared launcher: replace the link rather
        // than write through it. A link that stays (one the OS still holds on Windows, where the
        // shared file is writable) would carry this body into every stub sharing it, so stop.
        if let Err(error) = fs::remove_file(&path) {
            assert!(
                error.kind() == io::ErrorKind::NotFound,
                "cannot replace the earlier stub at {}: {error}",
                path.display()
            );
        }
        let linked =
            shared_launcher(&body).is_some_and(|shared| fs::hard_link(shared, &path).is_ok());
        if !linked {
            fs::write(&path, &body).unwrap();
            mark_executable(&path, PRIVATE_MODE).unwrap();
        }
        path
    }

    /// Write the launcher into `dir` and wrap it in an [`AgentCli`].
    pub fn cli(&self, dir: &Path) -> AgentCli {
        AgentCli::new(self.write(dir).to_string_lossy().as_ref())
    }

    fn body(&self) -> String {
        if cfg!(windows) {
            self.batch_body()
        } else {
            self.sh_body()
        }
    }

    fn batch_body(&self) -> String {
        let mut lines = vec![
            "@echo off".to_string(),
            format!("if defined {WARM_UP_VAR} exit /b 0"),
        ];
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
            format!("if [ -n \"${WARM_UP_VAR}\" ]; then exit 0; fi"),
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

/// The shared, already-started launcher holding `body`, or `None` when it cannot be shared.
///
/// Each body is published and warmed once per process; a thread that wants a launcher another
/// thread is still warming waits for it rather than starting it cold.
fn shared_launcher(body: &str) -> Option<PathBuf> {
    static READY: Mutex<BTreeMap<PathBuf, Arc<OnceLock<bool>>>> = Mutex::new(BTreeMap::new());
    let path = shared_launcher_path(body);
    let ready = Arc::clone(
        READY
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(path.clone())
            .or_default(),
    );
    ready
        .get_or_init(|| publish(&path, body).is_ok() && warm_up(&path))
        .then_some(path)
}

fn shared_launcher_path(body: &str) -> PathBuf {
    let os = std::env::consts::OS;
    let key = crate::sha::sha256_hex(format!("{os}\n{body}").as_bytes());
    std::env::temp_dir()
        .join("on-n-off-cli-stubs")
        .join(launcher_file_name(&key[..32]))
}

/// Makes `path` hold exactly `body` without ever showing a partly written file: the launcher is
/// staged under a name of this process's own and linked into place, so a test run in another
/// worktree either finds the whole file or publishes an identical one. A file already there is
/// used only if it holds this very body.
fn publish(path: &Path, body: &str) -> io::Result<()> {
    if !path.exists() {
        let dir = path
            .parent()
            .expect("a shared launcher lives in a directory");
        fs::create_dir_all(dir)?;
        let staging = dir.join(format!(
            "{}.{}.staging",
            path.file_name().unwrap_or_default().to_string_lossy(),
            std::process::id()
        ));
        let _ = fs::remove_file(&staging);
        let staged = fs::write(&staging, body)
            .and_then(|()| mark_executable(&staging, SHARED_MODE))
            .and_then(|()| fs::hard_link(&staging, path));
        let _ = fs::remove_file(&staging);
        match staged {
            Ok(()) => {}
            // Another test run published it first.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    if fs::read(path)? == body.as_bytes() {
        Ok(())
    } else {
        Err(io::Error::other("another file holds this launcher's name"))
    }
}

/// Starts the launcher once, doing nothing, so whatever a first start costs is paid here and not
/// inside the test that runs it next.
fn warm_up(path: &Path) -> bool {
    Command::new(path)
        .env(WARM_UP_VAR, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
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
fn mark_executable(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn mark_executable(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
