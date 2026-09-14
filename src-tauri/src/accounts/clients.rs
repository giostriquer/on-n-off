//! Conservative native-writer preflight. Processes are inspected only, never terminated.
use crate::dto::AgentId;
use std::{process::Command, time::Duration};
pub fn require_closed(provider: AgentId) -> Result<(), String> {
    #[cfg(unix)]
    let mut command = {
        let mut c = Command::new("/bin/ps");
        c.args(["-A", "-o", "args="]);
        c
    };
    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("powershell.exe");
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$ErrorActionPreference='Stop'; Get-CimInstance Win32_Process | ForEach-Object { $_.Name + ' ' + $_.CommandLine }",
        ]);
        c
    };
    let output = super::native::run(&mut command, Duration::from_secs(10))
        .map_err(|_| "Could not check running clients. Close provider clients and retry.")?;
    if output.lines().any(|line| conflicts(line, provider)) {
        return Err("Close this provider's CLI, desktop app and IDE agent sessions before changing its native login. on-n-off will not stop them for you.".into());
    }
    Ok(())
}
fn conflicts(line: &str, provider: AgentId) -> bool {
    let name = if provider == AgentId::Claude {
        "claude"
    } else {
        "codex"
    };
    line.split_whitespace().any(|part| {
        let part = part
            .trim_matches(['\'', '"'])
            .replace('\\', "/")
            .to_lowercase();
        let base = part.rsplit('/').next().unwrap_or("");
        base == name
            || base == format!("{name}.exe")
            || base == format!("{name}.js")
            || part.contains(&format!("/{name}.app/"))
            || part.contains(&format!("/{name}-code/"))
    })
}

#[cfg(test)]
mod tests;
