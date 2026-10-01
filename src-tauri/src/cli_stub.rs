use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use crate::cli::AgentCli;

pub const ANSWER_DEADLINE: Duration = Duration::from_secs(60);

const CHATTY_PAYLOAD: &str = "abcdefghijklmnopqrstuvwxyz0123456789";

const WARM_UP_VAR: &str = "ON_N_OFF_STUB_WARMUP";

const PRIVATE_MODE: u32 = 0o755;
const SHARED_MODE: u32 = 0o555;

#[derive(Debug, Default)]
pub struct CliStub {
    name: String,
    copy: Option<(String, String)>,
    args_log: Option<(String, bool)>,
    env_log: Option<(String, String)>,
    chatty_lines: usize,
    print_env: Option<String>,
    stdout: Option<String>,
    stdout_file: Option<String>,
    stderr: Option<String>,
    sleep_secs: u32,
    exit: i32,
}

impl CliStub {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Self::default()
        }
    }

    pub fn copy(mut self, from: &str, to: &str) -> Self {
        self.copy = Some((from.to_string(), to.to_string()));
        self
    }

    pub fn log_args(mut self, file: &str, append: bool) -> Self {
        self.args_log = Some((file.to_string(), append));
        self
    }

    pub fn log_env(mut self, name: &str, file: &str) -> Self {
        self.env_log = Some((name.to_string(), file.to_string()));
        self
    }

    pub fn chatty(mut self, lines: usize) -> Self {
        self.chatty_lines = lines;
        self
    }

    pub fn print_env(mut self, name: &str) -> Self {
        self.print_env = Some(name.to_string());
        self
    }

    pub fn stdout(mut self, text: &str) -> Self {
        self.stdout = Some(text.to_string());
        self
    }

    pub fn stdout_file(mut self, file: &str) -> Self {
        self.stdout_file = Some(file.to_string());
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

    pub fn write(&self, dir: &Path) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(launcher_file_name(&self.name));
        let body = self.body();
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
        if let Some((name, file)) = &self.env_log {
            lines.push(format!(
                "echo %{name}%> \"%~dp0{}\"",
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
        if let Some(file) = &self.stdout_file {
            lines.push(format!("type \"%~dp0{}\"", windows_relative(file)));
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
        if let Some((name, file)) = &self.env_log {
            lines.push(format!("printf '%s\\n' \"${name}\" > \"$here/{file}\""));
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
        if let Some(file) = &self.stdout_file {
            lines.push(format!("cat \"$here/{file}\""));
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
