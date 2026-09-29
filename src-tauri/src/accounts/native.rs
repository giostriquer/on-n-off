//! What both adapters' native stores share: running a provider's official CLI, reading native
//! configuration, and the refusals every account-change preflight makes. No whole-home restores
//! and no provider endpoint/config rewriting; each provider's own rules live in its adapter
//! (`claude.rs`, `codex.rs`).
use crate::{
    dto::AgentId,
    process::{wait_with_deadline, CommandOutcome},
};
use serde_json::{json, Value};
use std::{
    ffi::OsString,
    fs,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

/// What account changes say about a native home the environment chose.
pub(super) const CUSTOM_HOME: &str = "Account activation currently supports the default CLI home. Remove the custom home override or use the official CLI for this context.";

/// Refuses a configuration file that is a link: the official client changes it, not on-n-off.
pub(super) fn refuse_linked(config_file: &Path) -> Result<(), String> {
    if fs::symlink_metadata(config_file).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(
            "Linked account configuration must be changed through the official CLI.".into(),
        );
    }
    Ok(())
}

/// Refuses when the environment `env` reads sets any of `names`, a credential that overrides
/// the native login.
pub(super) fn refuse_env_credentials(
    names: &[&str],
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<(), String> {
    if names.iter().any(|name| env(name).is_some()) {
        return Err("An environment credential overrides native login. Remove the override before using saved profiles.".into());
    }
    Ok(())
}

/// The provider's official CLI, `name`, found as a GUI app must find it.
pub(super) fn cli(provider: AgentId, name: &str) -> Command {
    #[cfg(test)]
    if let Some(program) = TEST_CLI.with(|cli| cli.borrow().clone()) {
        return crate::cli::AgentCli::new(program).command();
    }
    let binary = crate::cli_locate::resolve_provider_cli(provider, name)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.into());
    crate::cli::AgentCli::new(binary).command()
}

#[cfg(test)]
thread_local! {
    static TEST_CLI: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Runs `run` with every official CLI this thread's `cli` finds replaced by `program`, a stub, so a
/// real store can be driven without a real client.
#[cfg(test)]
pub(crate) fn with_test_cli<T>(program: &Path, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_CLI.with(|cli| *cli.borrow_mut() = self.0.take());
        }
    }
    let previous = TEST_CLI.with(|cli| cli.replace(Some(program.to_string_lossy().into_owned())));
    let _restore = Restore(previous);
    run()
}

pub fn read_json(path: &Path) -> Result<Value, String> {
    match fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "Native configuration is malformed.".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(_) => Err("Cannot read native configuration.".into()),
    }
}

pub fn run(command: &mut Command, timeout: Duration) -> Result<String, String> {
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "Could not start the official CLI.")?;
    match wait_with_deadline(child, timeout) {
        Ok(CommandOutcome::Exited {
            success: true,
            stdout,
            ..
        }) => Ok(stdout),
        _ => Err("The official CLI did not complete the account operation.".into()),
    }
}
