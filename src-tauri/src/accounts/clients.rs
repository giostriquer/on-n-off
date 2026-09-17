//! Conservative native-writer preflight. Processes are inspected only, never terminated.
use crate::dto::AgentId;
use std::{process::Command, time::Duration};

/// A running process: what it was started as, and its whole command line. Only the executable
/// (or the script a JavaScript runtime launched) identifies a client, so crash handlers under a
/// framework named after the provider and tools that merely pass its name as an argument do not.
#[derive(Debug, PartialEq)]
struct Process {
    executable: String,
    args: String,
}

const SCRIPT_HOSTS: [&str; 6] = ["node", "node.exe", "bun", "bun.exe", "deno", "deno.exe"];

/// Ordinary activation can rely on Claude Code's native credential-change handling.
/// Sign-out, crash recovery, and abandoned-login cleanup still require closed clients.
pub fn require_activation_safe(provider: AgentId) -> Result<(), String> {
    activation_preflight(provider, require_closed)
}

fn activation_preflight(
    provider: AgentId,
    check_closed: impl FnOnce(AgentId) -> Result<(), String>,
) -> Result<(), String> {
    if provider == AgentId::Claude {
        return Ok(());
    }
    check_closed(provider)
}

pub fn require_closed(provider: AgentId) -> Result<(), String> {
    let processes = running_processes()
        .map_err(|_| "Could not check running clients. Close provider clients and retry.")?;
    if processes.iter().any(|process| conflicts(process, provider)) {
        return Err("Close this provider's CLI, desktop app and IDE agent sessions before changing its native login. on-n-off will not stop them for you.".into());
    }
    Ok(())
}

#[cfg(unix)]
fn running_processes() -> Result<Vec<Process>, String> {
    let list = |column: &str| {
        let mut command = Command::new("/bin/ps");
        command.args(["-A", "-o", &format!("pid=,{column}=")]);
        super::native::run(&mut command, Duration::from_secs(10))
    };
    Ok(ps_processes(&list("comm")?, &list("args")?))
}

#[cfg(windows)]
fn running_processes() -> Result<Vec<Process>, String> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "$ErrorActionPreference='Stop'; Get-CimInstance Win32_Process | ForEach-Object { [string]$_.ExecutablePath + [char]9 + $_.Name + [char]9 + $_.CommandLine }",
    ]);
    Ok(cim_processes(&super::native::run(
        &mut command,
        Duration::from_secs(10),
    )?))
}

/// Joins `ps -o pid=,comm=` and `ps -o pid=,args=` by pid. `comm` keeps spaces in the executable.
#[cfg(any(unix, test))]
fn ps_processes(executables: &str, args: &str) -> Vec<Process> {
    let rows = |output: &str| {
        output
            .lines()
            .filter_map(|line| line.trim_start().split_once(' '))
            .map(|(pid, rest)| (pid.to_owned(), rest.trim_start().to_owned()))
            .collect::<std::collections::HashMap<_, _>>()
    };
    let mut args = rows(args);
    rows(executables)
        .into_iter()
        .map(|(pid, executable)| Process {
            args: args.remove(&pid).unwrap_or_default(),
            executable,
        })
        .collect()
}

/// Parses `ExecutablePath<TAB>Name<TAB>CommandLine`; the path is empty for protected processes.
#[cfg(any(windows, test))]
fn cim_processes(output: &str) -> Vec<Process> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut fields = line.splitn(3, '\t');
            let path = fields.next().unwrap_or_default();
            let name = fields.next().unwrap_or_default();
            Process {
                executable: if path.is_empty() { name } else { path }.to_owned(),
                args: fields.next().unwrap_or_default().to_owned(),
            }
        })
        .collect()
}

fn conflicts(process: &Process, provider: AgentId) -> bool {
    let name = if provider == AgentId::Claude {
        "claude"
    } else {
        "codex"
    };
    let executable = normalized(&process.executable);
    let base = executable.rsplit('/').next().unwrap_or("");
    base == name
        || base == format!("{name}.exe")
        || (SCRIPT_HOSTS.contains(&base) && launches_script(process, name))
}

/// The script is the first argument after the runtime's own flags. Its path may contain spaces,
/// so it grows one token at a time until it names the provider, ends in a JavaScript file, or
/// reaches the next flag.
fn launches_script(process: &Process, name: &str) -> bool {
    let args = process.args.trim();
    let rest = args
        .strip_prefix(process.executable.as_str())
        .or_else(|| match args.strip_prefix('"') {
            Some(quoted) => quoted.split_once('"').map(|(_, rest)| rest),
            None => args.split_once(char::is_whitespace).map(|(_, rest)| rest),
        })
        .unwrap_or_default();
    let mut tokens = rest
        .split_whitespace()
        .skip_while(|token| token.starts_with('-'))
        .peekable();
    let mut script = String::new();
    while let Some(token) = tokens.next() {
        if !script.is_empty() {
            script.push(' ');
        }
        script.push_str(token);
        let path = normalized(&script);
        let base = path.rsplit('/').next().unwrap_or("");
        if base == name || base == format!("{name}.js") {
            return true;
        }
        if [".js", ".mjs", ".cjs"]
            .iter()
            .any(|ext| base.ends_with(ext))
        {
            return path.contains(&format!("/{name}-code/"));
        }
        if tokens.peek().is_none_or(|next| next.starts_with('-')) {
            return false;
        }
    }
    false
}

fn normalized(path: &str) -> String {
    path.trim_matches(['\'', '"'])
        .replace('\\', "/")
        .to_lowercase()
}

#[cfg(test)]
mod tests;
