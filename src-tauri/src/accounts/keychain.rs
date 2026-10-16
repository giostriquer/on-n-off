#[cfg(any(target_os = "macos", test))]
use crate::process::CommandOutcome;

#[cfg(target_os = "macos")]
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(20);

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

#[cfg(target_os = "macos")]
pub(super) fn delete(service: &str, account: &str) -> Result<(), String> {
    let command = delete_command(service, account).map_err(|reason| failed("delete", &reason))?;
    match run(&command) {
        Err(reason) if not_found(&reason) => Ok(()),
        outcome => outcome.map_err(|reason| failed("delete", &reason)),
    }
}

#[cfg(target_os = "macos")]
fn failed(operation: &str, reason: &str) -> String {
    format!(
        "Keychain {operation} failed ({}).",
        reason.trim_end_matches('.')
    )
}

#[cfg(any(target_os = "macos", test))]
fn not_found(reason: &str) -> bool {
    reason.contains("-25300") || reason.contains("could not be found")
}

#[cfg(any(target_os = "macos", test))]
fn write_command(service: &str, account: &str, raw: &[u8]) -> Result<String, String> {
    let hex: String = raw.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!(
        "add-generic-password -U -a {} -s {} -X \"{hex}\"\n",
        quoted(account)?,
        quoted(service)?
    ))
}

#[cfg(any(target_os = "macos", test))]
fn delete_command(service: &str, account: &str) -> Result<String, String> {
    Ok(format!(
        "delete-generic-password -a {} -s {}\n",
        quoted(account)?,
        quoted(service)?
    ))
}

#[cfg(any(target_os = "macos", test))]
fn quoted(identifier: &str) -> Result<String, String> {
    if identifier.is_empty() || identifier.contains(['"', '\\', '\n', '\r']) {
        return Err(format!(
            "Keychain identifier {identifier:?} cannot be quoted"
        ));
    }
    Ok(format!("\"{identifier}\""))
}

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

#[cfg(target_os = "macos")]
fn run(command: &str) -> Result<(), String> {
    #[cfg(test)]
    if let Some(answer) = test_answer(command) {
        return interpret(answer);
    }
    spawn(command).and_then(interpret)
}

#[cfg(target_os = "macos")]
const PASSWORD_DEADLINE: std::time::Duration = std::time::Duration::from_secs(90);

#[cfg(target_os = "macos")]
pub(super) fn find_password(
    service: &str,
    account: Option<&str>,
) -> Result<Option<String>, String> {
    let mut args = vec!["find-generic-password"];
    if let Some(account) = account {
        args.extend(["-a", account]);
    }
    args.extend(["-w", "-s", service]);
    match find(&args, PASSWORD_DEADLINE) {
        Ok(CommandOutcome::Exited {
            success,
            stdout,
            stderr,
        }) => interpret_password(success, &stdout, &stderr),
        Ok(CommandOutcome::TimedOut) => Err("Keychain prompt was not answered in time".to_string()),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "macos")]
pub(super) fn find_attributes(
    service: &str,
    account: Option<&str>,
    deadline: std::time::Duration,
) -> Result<CommandOutcome, String> {
    let mut args = vec!["find-generic-password"];
    if let Some(account) = account {
        args.extend(["-a", account]);
    }
    args.extend(["-s", service]);
    find(&args, deadline)
}

#[cfg(any(target_os = "macos", test))]
fn interpret_password(success: bool, stdout: &str, stderr: &str) -> Result<Option<String>, String> {
    if success {
        let secret = stdout.trim();
        return Ok((!secret.is_empty()).then(|| secret.to_string()));
    }
    let detail = stderr.trim();
    if detail.contains("could not be found") {
        return Ok(None);
    }
    let detail = if detail.is_empty() {
        "no details".to_string()
    } else {
        detail.to_string()
    };
    Err(format!(
        "Keychain lookup failed ({detail}). If macOS asked to allow access, click Allow and refresh."
    ))
}

#[cfg(target_os = "macos")]
fn find(args: &[&str], deadline: std::time::Duration) -> Result<CommandOutcome, String> {
    use std::process::{Command, Stdio};

    #[cfg(test)]
    if let Some(answer) = test_answer(&args.join(" ")) {
        return Ok(answer);
    }
    let child = Command::new("/usr/bin/security")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not run /usr/bin/security: {error}"))?;
    crate::process::wait_with_deadline(child, deadline)
        .map_err(|error| format!("Keychain lookup failed: {error}"))
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
type Answer = std::rc::Rc<dyn Fn(&str) -> CommandOutcome>;

#[cfg(all(target_os = "macos", test))]
thread_local! {
    static TEST_RUNNER: std::cell::RefCell<Option<Answer>> = const { std::cell::RefCell::new(None) };
    static SENT: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    static REAL_KEYCHAIN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(all(target_os = "macos", test))]
fn test_answer(command: &str) -> Option<CommandOutcome> {
    let Some(runner) = TEST_RUNNER.with(|slot| slot.borrow().clone()) else {
        assert!(
            REAL_KEYCHAIN.with(std::cell::Cell::get),
            "a test reached the real Keychain: security {command}"
        );
        return None;
    };
    SENT.with(|sent| sent.borrow_mut().push(command.to_string()));
    Some(runner(command))
}

#[cfg(all(target_os = "macos", test))]
pub(crate) fn with_real_keychain<T>(run: impl FnOnce() -> T) -> T {
    REAL_KEYCHAIN.with(|flag| flag.set(true));
    let result = run();
    REAL_KEYCHAIN.with(|flag| flag.set(false));
    result
}

#[cfg(all(target_os = "macos", test))]
pub(super) fn with_test_runner<T>(
    runner: impl Fn(&str) -> CommandOutcome + 'static,
    run: impl FnOnce() -> T,
) -> (T, Vec<String>) {
    TEST_RUNNER.with(|slot| *slot.borrow_mut() = Some(std::rc::Rc::new(runner)));
    SENT.with(|sent| sent.borrow_mut().clear());
    let result = run();
    TEST_RUNNER.with(|slot| *slot.borrow_mut() = None);
    (result, SENT.with(std::cell::RefCell::take))
}

#[cfg(all(target_os = "macos", test))]
pub(super) fn fake_items(
    items: &'static [(&'static str, &'static str)],
) -> impl Fn(&str) -> CommandOutcome + 'static {
    move |command| {
        let exited = |success: bool, stdout: String, stderr: &str| CommandOutcome::Exited {
            success,
            stdout,
            stderr: stderr.to_string(),
        };
        if !command.starts_with("find-generic-password") {
            return exited(true, String::new(), "");
        }
        let named = command
            .split_once(" -a ")
            .and_then(|(_, rest)| rest.split(' ').next());
        let item = match named {
            Some(account) => items.iter().find(|(filed, _)| *filed == account),
            None => items.last(),
        };
        match (item, command.contains(" -w ")) {
            (None, _) => exited(
                false,
                String::new(),
                "security: SecKeychainSearchCopyNext: The specified item could not be found in the keychain.",
            ),
            (Some((account, _)), false) => {
                exited(true, format!("    \"acct\"<blob>=\"{account}\"\n"), "")
            }
            (Some((_, secret)), true) => exited(true, format!("{secret}\n"), ""),
        }
    }
}

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
