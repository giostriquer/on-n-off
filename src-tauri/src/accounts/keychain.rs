//! The one way on-n-off changes a macOS Keychain item: `/usr/bin/security`, never this process.
//!
//! Every provider item the app touches — Claude Code's login, Codex's keyring login and the
//! scoped entry of an isolated sign-in — it already reads through `security`
//! (`limits::credentials`), so the identity those items trust is the tool's, which Apple signs.
//! This process is ad-hoc signed: its identity is the hash of one build, so a trusted-application
//! entry for it expires with every update, and an in-process write moves the item's partition
//! list onto that hash, after which the next `security` read asks the user again. Two identities
//! taking turns on one item is what turned "Always Allow" into a prompt on every account switch.
//! One identity, the tool's, keeps a single "Always Allow" good across updates and across Claude
//! Code's own refreshes, which write the same way.
//!
//! The vault key (`vault.rs`) is the deliberate exception: it is the app's own item, the one the
//! app should have to prove itself for, at the cost of one prompt per app run on a new build.
//!
//! Argument order is `service, account`, as the reads in `limits::credentials` take them.

#[cfg(any(target_os = "macos", test))]
use crate::process::CommandOutcome;

/// Deadline for one `security` call. Renewal writes under Claude Code's refresh locks, which it
/// holds without a heartbeat and which Claude Code breaks after a minute, so this sits comfortably
/// inside that: a write that outlives the lock is a write racing whoever broke it; `claude_renew`
/// pins the relation. Activation's native locks are heartbeated and put no such bound on it.
#[cfg(target_os = "macos")]
pub(super) const DEADLINE: std::time::Duration = std::time::Duration::from_secs(20);

/// Store `raw` as the secret of the item filed under `service` and `account`, replacing an entry
/// that already exists.
#[cfg(target_os = "macos")]
pub(super) fn write(service: &str, account: &str, raw: &[u8]) -> Result<(), String> {
    write_command(service, account, raw)
        .and_then(|command| run(&command))
        .map_err(|reason| failed("write", &reason))
}

#[cfg(not(target_os = "macos"))]
pub(super) fn write(_service: &str, _account: &str, _raw: &[u8]) -> Result<(), String> {
    Err("This platform has no Keychain.".to_string())
}

/// Remove the item. An item that is already gone is what a removal asks for, not an error.
#[cfg(target_os = "macos")]
pub(super) fn delete(service: &str, account: &str) -> Result<(), String> {
    let command = delete_command(service, account).map_err(|reason| failed("delete", &reason))?;
    // Only the tool's own answer reaches `not_found`; a refused identifier never looks like one.
    match run(&command) {
        Err(reason) if not_found(&reason) => Ok(()),
        outcome => outcome.map_err(|reason| failed("delete", &reason)),
    }
}

/// A terminated sentence, because the transaction prefixes its own to it.
#[cfg(target_os = "macos")]
fn failed(operation: &str, reason: &str) -> String {
    format!(
        "Keychain {operation} failed ({}).",
        reason.trim_end_matches('.')
    )
}

/// `security` answers a missing item with `errSecItemNotFound` (-25300) and its English text;
/// either one is the code, not the tool, so both are matched.
#[cfg(any(target_os = "macos", test))]
fn not_found(reason: &str) -> bool {
    reason.contains("-25300") || reason.contains("could not be found")
}

/// `-U` replaces an existing entry in place, keeping its access list, and `-X` takes the secret
/// hex-encoded, as Claude Code writes it, so no byte of it can escape into the command.
#[cfg(any(target_os = "macos", test))]
fn write_command(service: &str, account: &str, raw: &[u8]) -> Result<String, String> {
    let hex: String = raw.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!(
        "add-generic-password -U -a {} -s {} -X \"{hex}\"\n",
        quoted(account)?,
        quoted(service)?
    ))
}

/// Service *and* account: Codex files every home's login under one service, told apart by
/// account, so a service-only delete could take another home's login with it.
#[cfg(any(target_os = "macos", test))]
fn delete_command(service: &str, account: &str) -> Result<String, String> {
    Ok(format!(
        "delete-generic-password -a {} -s {}\n",
        quoted(account)?,
        quoted(service)?
    ))
}

/// Identifiers come from `security`'s own attribute dump, a `cli|<hash>` Codex account or a
/// constant service, none of which need escaping. One that would is refused rather than guessed
/// at: `security -i` documents no escape, and a wrong guess files the secret under a name nothing
/// reads.
#[cfg(any(target_os = "macos", test))]
fn quoted(identifier: &str) -> Result<String, String> {
    if identifier.is_empty() || identifier.contains(['"', '\\', '\n', '\r']) {
        return Err(format!(
            "Keychain identifier {identifier:?} cannot be quoted"
        ));
    }
    Ok(format!("\"{identifier}\""))
}

/// The tool's own words, on one line, for the caller to name the operation.
#[cfg(any(target_os = "macos", test))]
fn interpret(outcome: CommandOutcome) -> Result<(), String> {
    match outcome {
        CommandOutcome::Exited { success: true, .. } => Ok(()),
        CommandOutcome::Exited { stderr, .. } => {
            let reason = stderr
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            Err(if reason.is_empty() {
                "security exited with an error".to_string()
            } else {
                reason
            })
        }
        CommandOutcome::TimedOut => Err("not answered in time".to_string()),
    }
}

/// One `security -i` command, sent on stdin: a secret on the command line would sit in the
/// process table for every other user on the machine to read. Under test the command goes to the
/// runner `with_test_runner` installed instead, so a test can see what would have been sent
/// without a Keychain being touched.
#[cfg(target_os = "macos")]
fn run(command: &str) -> Result<(), String> {
    #[cfg(test)]
    if let Some(runner) = TEST_RUNNER.with(|slot| *slot.borrow()) {
        SENT.with(|sent| sent.borrow_mut().push(command.to_string()));
        return interpret(runner(command));
    }
    spawn(command).and_then(interpret)
}

#[cfg(target_os = "macos")]
fn spawn(command: &str) -> Result<CommandOutcome, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("/usr/bin/security")
        .arg("-i")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not run /usr/bin/security: {error}"))?;
    // Bound and dropped explicitly: `security` reads commands until stdin closes, so the handle
    // has to go before the wait. One command of a few kilobytes cannot fill a pipe buffer, so
    // writing it before the drainers start cannot deadlock.
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "security took no stdin".to_string())?;
    let written = stdin.write_all(command.as_bytes());
    drop(stdin);
    written.map_err(|error| format!("could not write to security: {error}"))?;
    crate::process::wait_with_deadline(child, DEADLINE).map_err(|error| error.to_string())
}

#[cfg(all(target_os = "macos", test))]
pub(super) type Runner = fn(&str) -> CommandOutcome;

#[cfg(all(target_os = "macos", test))]
thread_local! {
    static TEST_RUNNER: std::cell::RefCell<Option<Runner>> = const { std::cell::RefCell::new(None) };
    static SENT: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Run `run` with every `security` command answered by `runner` instead of the tool. Returns
/// `run`'s result and the commands that would have been sent, in order.
#[cfg(all(target_os = "macos", test))]
pub(super) fn with_test_runner<T>(runner: Runner, run: impl FnOnce() -> T) -> (T, Vec<String>) {
    TEST_RUNNER.with(|slot| *slot.borrow_mut() = Some(runner));
    SENT.with(|sent| sent.borrow_mut().clear());
    let result = run();
    TEST_RUNNER.with(|slot| *slot.borrow_mut() = None);
    (result, SENT.with(std::cell::RefCell::take))
}

/// A throwaway entry for the rehearsals that really drive `security`, removed on drop by the tool
/// directly, so a failing assertion leaves nothing behind in the login Keychain.
#[cfg(all(target_os = "macos", test))]
pub(super) struct ThrowawayEntry {
    pub(super) service: &'static str,
    pub(super) account: &'static str,
}

#[cfg(all(target_os = "macos", test))]
impl Drop for ThrowawayEntry {
    fn drop(&mut self) {
        std::process::Command::new("/usr/bin/security")
            .args([
                "delete-generic-password",
                "-a",
                self.account,
                "-s",
                self.service,
            ])
            .output()
            .ok();
    }
}

#[cfg(test)]
mod tests;
