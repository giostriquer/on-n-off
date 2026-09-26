//! Conservative native-writer preflight. Processes are inspected only, never terminated.
use crate::dto::AgentId;
use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
    process::Command,
    time::Duration,
};

/// A running process: what it was started as, its parent, and its whole command line. Only the
/// executable (or the script a JavaScript runtime launched) identifies a client, so crash handlers
/// under a framework named after the provider and tools that merely pass its name do not.
#[derive(Debug, PartialEq)]
struct Process {
    pid: String,
    parent: String,
    executable: String,
    args: String,
}

/// How a provider's clients are told apart from every other process, and whether they refuse an
/// ordinary switch. Each adapter holds its provider's.
pub(super) struct Client {
    /// The executable a client runs as, and the script a JavaScript runtime launches for it.
    pub(super) name: &'static str,
    /// The package's own entry script, which identifies a client whatever runs it.
    pub(super) package_entry: &'static str,
    /// Whether an ordinary switch refuses to run beside these clients: not when they handle a
    /// native credential change themselves.
    pub(super) blocks_activation: bool,
}

const SCRIPT_HOSTS: [&str; 6] = ["node", "node.exe", "bun", "bun.exe", "deno", "deno.exe"];
const SCRIPT_EXTENSIONS: [&str; 8] = [".js", ".mjs", ".cjs", ".jsx", ".ts", ".mts", ".cts", ".tsx"];

/// Names the clients an ordinary switch refuses to run beside, so a person can close them or
/// switch anyway: none for a provider whose clients take a native credential change.
///
/// This scan runs before the account-change lease, while on-n-off's own provider reads may be
/// live, so it leaves out processes on-n-off started. The checks that gate a change never do:
/// Windows keeps a dead parent's pid on its children and reuses pids, so the exclusion could hide
/// a real client there, while the lease already keeps on-n-off's own reads from running.
pub fn activation_blockers(provider: AgentId) -> Result<Vec<String>, String> {
    blockers(
        super::adapter(provider)?.client(),
        running_processes,
        Some(&std::process::id().to_string()),
    )
}

/// Sign-out, crash recovery, and abandoned-login cleanup still require closed clients.
pub fn require_activation_safe(provider: AgentId) -> Result<(), String> {
    closed(&blockers(
        super::adapter(provider)?.client(),
        running_processes,
        None,
    )?)
}

pub fn require_closed(provider: AgentId) -> Result<(), String> {
    closed(&running_clients(
        super::adapter(provider)?.client(),
        running_processes,
        None,
    )?)
}

fn blockers(
    client: &Client,
    scan: impl FnOnce() -> Result<Vec<Process>, String>,
    own: Option<&str>,
) -> Result<Vec<String>, String> {
    if !client.blocks_activation {
        return Ok(Vec::new());
    }
    running_clients(client, scan, own)
}

fn closed(clients: &[String]) -> Result<(), String> {
    if clients.is_empty() {
        return Ok(());
    }
    Err(format!("Close this provider's CLI, desktop app and IDE agent sessions before changing its native login: {}. on-n-off will not stop them for you.", clients.join(", ")))
}

fn running_clients(
    client: &Client,
    scan: impl FnOnce() -> Result<Vec<Process>, String>,
    own: Option<&str>,
) -> Result<Vec<String>, String> {
    let processes =
        scan().map_err(|_| "Could not check running clients. Close provider clients and retry.")?;
    Ok(clients(&processes, client, own))
}

/// `own` leaves out the processes that pid started.
fn clients(processes: &[Process], client: &Client, own: Option<&str>) -> Vec<String> {
    let by_pid: HashMap<_, _> = processes.iter().map(|p| (p.pid.as_str(), p)).collect();
    processes
        .iter()
        .filter(|process| own.is_none_or(|own| !lineage(process, &by_pid).any(|p| p.pid == own)))
        .filter_map(|process| {
            let name = client_name(process, client)?;
            // Name a client after the app bundle it runs in or was started from, which is what
            // a person closes; one with no app around it keeps its own name.
            Some(
                lineage(process, &by_pid)
                    .enumerate()
                    .find_map(|(depth, p)| {
                        let bundle = p
                            .executable
                            .split(['/', '\\'])
                            .find_map(|part| part.strip_suffix(".app"))?;
                        Some(if depth == 0 {
                            bundle.to_owned()
                        } else {
                            format!("{bundle} ({name})")
                        })
                    })
                    .unwrap_or(name),
            )
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The process and its ancestors, bounded so reused pids cannot cycle forever.
fn lineage<'a>(
    process: &'a Process,
    by_pid: &'a HashMap<&str, &'a Process>,
) -> impl Iterator<Item = &'a Process> {
    std::iter::successors(Some(process), |p| {
        by_pid
            .get(p.parent.as_str())
            .copied()
            .filter(|parent| parent.pid != p.pid)
    })
    .take(by_pid.len().max(1))
}

#[cfg(unix)]
fn running_processes() -> Result<Vec<Process>, String> {
    let list = |columns: &str| {
        let mut command = Command::new("/bin/ps");
        command.args(["-A", "-o", columns]);
        super::native::run(&mut command, Duration::from_secs(10))
    };
    Ok(ps_processes(
        &list("pid=,ppid=,comm=")?,
        &list("pid=,args=")?,
    ))
}

#[cfg(windows)]
fn running_processes() -> Result<Vec<Process>, String> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "$ErrorActionPreference='Stop'; Get-CimInstance Win32_Process | ForEach-Object { [string]$_.ProcessId + [char]9 + [string]$_.ParentProcessId + [char]9 + [string]$_.ExecutablePath + [char]9 + $_.Name + [char]9 + $_.CommandLine }",
    ]);
    Ok(cim_processes(&super::native::run(
        &mut command,
        Duration::from_secs(10),
    )?))
}

/// Joins `ps -o pid=,ppid=,comm=` and `ps -o pid=,args=` by pid. `comm` is argv[0] with its spaces.
#[cfg(any(unix, test))]
fn ps_processes(executables: &str, args: &str) -> Vec<Process> {
    let rows = |output: &str| {
        output
            .lines()
            .filter_map(|line| line.trim_start().split_once(' '))
            .map(|(pid, rest)| (pid.to_owned(), rest.trim_start().to_owned()))
            .collect::<HashMap<_, _>>()
    };
    let mut args = rows(args);
    rows(executables)
        .into_iter()
        .filter_map(|(pid, rest)| {
            let (parent, executable) = rest.split_once(' ')?;
            Some(Process {
                args: args.remove(&pid).unwrap_or_default(),
                parent: parent.to_owned(),
                executable: executable.trim_start().to_owned(),
                pid,
            })
        })
        .collect()
}

/// Parses `ProcessId<TAB>ParentProcessId<TAB>ExecutablePath<TAB>Name<TAB>CommandLine`; the path
/// is empty for protected processes.
#[cfg(any(windows, test))]
fn cim_processes(output: &str) -> Vec<Process> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut fields = line.splitn(5, '\t').map(str::to_owned);
            let mut next = || fields.next().unwrap_or_default();
            let (pid, parent, path, name) = (next(), next(), next(), next());
            Process {
                pid,
                parent,
                executable: if path.is_empty() { name } else { path },
                args: next(),
            }
        })
        .collect()
}

/// The name a provider client goes by, or `None` for any other process.
fn client_name(process: &Process, client: &Client) -> Option<String> {
    let name = client.name;
    let file = process.executable.rsplit(['/', '\\']).next()?;
    let base = normalized(file);
    if base == name || base == format!("{name}.exe") {
        return Some(file.to_owned());
    }
    let script = SCRIPT_HOSTS.contains(&base.as_str()) && launches_script(process, name);
    (script || runs_package_entry(process, client)).then(|| name.to_owned())
}

/// The package's own entry script identifies a client whatever runs it, including an Electron
/// helper acting as Node.
fn runs_package_entry(process: &Process, client: &Client) -> bool {
    process
        .args
        .split_whitespace()
        .any(|token| normalized(token).ends_with(client.package_entry))
}

/// The script is the first argument after the runtime's own flags. Windows quotes a path with
/// spaces; `ps` does not, so there the script ends at the first prefix that is a file or has a
/// script extension. Without either, only the first token can name it.
fn launches_script(process: &Process, name: &str) -> bool {
    let args = process.args.trim();
    let mut rest = args
        .strip_prefix(process.executable.as_str())
        .or_else(|| match args.strip_prefix('"') {
            Some(quoted) => quoted.split_once('"').map(|(_, rest)| rest),
            None => args.split_once(char::is_whitespace).map(|(_, rest)| rest),
        })
        .unwrap_or_default()
        .trim_start();
    while rest.starts_with('-') {
        rest = rest
            .split_once(char::is_whitespace)
            .map_or("", |(_, rest)| rest)
            .trim_start();
    }
    if let Some(quoted) = rest.strip_prefix('"') {
        return names(quoted.split('"').next().unwrap_or_default(), name);
    }
    let mut tokens = rest.split_whitespace().peekable();
    let first = tokens.peek().copied().unwrap_or_default();
    let mut script = String::new();
    while let Some(token) = tokens.next() {
        if !script.is_empty() {
            script.push(' ');
        }
        script.push_str(token);
        let path = normalized(&script);
        if Path::new(&script).is_file() || SCRIPT_EXTENSIONS.iter().any(|ext| path.ends_with(ext)) {
            return names(&script, name);
        }
        if tokens.peek().is_none_or(|next| next.starts_with('-')) {
            break;
        }
    }
    names(first, name)
}

fn names(script: &str, name: &str) -> bool {
    let path = normalized(script);
    let base = path.rsplit('/').next().unwrap_or("");
    base == name || base == format!("{name}.js")
}

fn normalized(path: &str) -> String {
    path.trim_matches(['\'', '"'])
        .replace('\\', "/")
        .to_lowercase()
}

#[cfg(test)]
mod tests;
