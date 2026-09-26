//! Narrow native-store access. No whole-home restores and no provider endpoint/config rewriting.
use super::{
    codex::CodexLogin,
    codex_store,
    model::{Identity, LoginView},
    store::Login,
    transaction::{Native, NativeGuard, ReadBack},
    IsolatedSignIn, NativeAccount,
};
use crate::{
    dto::{AgentId, ProviderLimitsDto},
    process::{wait_with_deadline, CommandOutcome},
};
use serde_json::{json, Value};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
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
    let binary = crate::cli_locate::resolve_provider_cli(provider, name)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.into());
    crate::cli::AgentCli::new(binary).command()
}

pub struct NativeStore {
    pub config_home: PathBuf,
    pub config_file: PathBuf,
    pub custom: bool,
}
impl NativeStore {
    pub fn resolve(home: &Path) -> Result<Self, String> {
        Self::resolve_from(home, &crate::paths::process_env)
    }
    fn resolve_from(
        home: &Path,
        lookup: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<Self, String> {
        // ON_N_OFF_HOME always isolates tests and development from real native homes.
        let disposable = lookup("ON_N_OFF_HOME").is_some();
        let override_home = if disposable {
            None
        } else {
            lookup("CODEX_HOME")
        };
        let custom = override_home.is_some();
        let config_home = override_home
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        if !config_home.is_absolute() {
            return Err("The provider home must be an absolute path.".into());
        }
        Ok(Self {
            config_file: codex_store::config_file(&config_home),
            config_home,
            custom,
        })
    }
    fn isolated(dir: &Path) -> Result<Self, String> {
        let config_home = dir.join(".codex");
        fs::create_dir_all(&config_home).map_err(|_| "Cannot create isolated login home.")?;
        Ok(Self {
            config_file: codex_store::config_file(&config_home),
            config_home,
            custom: true,
        })
    }
    pub fn preflight(&self) -> Result<(), String> {
        if self.custom {
            return Err(CUSTOM_HOME.into());
        }
        refuse_linked(&self.config_file)?;
        refuse_env_credentials(
            &["OPENAI_API_KEY", "CODEX_API_KEY", "CODEX_AUTH_TOKEN"],
            &crate::paths::process_env,
        )?;
        let config = codex_store::config(&self.config_home)?;
        for key in [
            "forced_login_method",
            "forced_chatgpt_workspace_id",
            "auth_keyring_backend",
            "profile",
        ] {
            if config.get(key).is_some() {
                return Err("This Codex installation enforces an authentication policy. Use its official sign-in controls.".into());
            }
        }
        if Path::new("/etc/codex/requirements.toml").exists()
            || Path::new("/etc/codex/managed_config.toml").exists()
        {
            return Err(
                "Managed Codex authentication must be changed through the official client.".into(),
            );
        }
        Ok(())
    }
    pub fn command(&self) -> Command {
        let mut command = cli(AgentId::Codex, "codex");
        command.env("CODEX_HOME", &self.config_home);
        command.current_dir(&self.config_home);
        command
    }
    fn verify_codex(&self, force: bool) -> Result<(), String> {
        let entries = crate::limits::read_limits(AgentId::Codex, force);
        if entries
            .iter()
            .any(|v| v.current_account && v.status == crate::dto::LimitsStatus::Ok)
        {
            Ok(())
        } else {
            Err("The CLI login could not be verified. Check connectivity or sign in again.".into())
        }
    }
}
impl Native for NativeStore {
    fn read(&self) -> Result<Option<Login>, String> {
        codex_store::read(&self.config_home)
    }
    fn identify(&self, login: &Login) -> Result<Identity, String> {
        CodexLogin::of(login).identity()
    }
    fn write(&self, login: Option<&Login>) -> Result<(), String> {
        self.write_locked(login, self.lock()?).map(drop)
    }
    fn write_locked(
        &self,
        login: Option<&Login>,
        locks: Box<dyn NativeGuard>,
    ) -> Result<ReadBack, String> {
        locks.ensure()?;
        codex_store::target(&self.config_home)?.write(login.map(|l| &l.auth))?;
        let back = self.read();
        drop(locks);
        Ok(back)
    }
    fn verify(&self) -> Result<(), String> {
        if self.custom {
            return run(
                self.command().args(["login", "status"]),
                Duration::from_secs(30),
            )
            .map(|_| ());
        }
        self.verify_codex(true)
    }
    fn renews_soon(&self, login: &Login) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |v| i64::try_from(v.as_secs()).unwrap_or(i64::MAX));
        CodexLogin::of(login).renews_soon(now)
    }
    fn verify_observed(&self) -> Result<(), String> {
        if self.custom {
            self.verify()
        } else {
            self.verify_codex(false)
        }
    }
}
impl NativeAccount for NativeStore {
    fn preflight(&self) -> Result<(), String> {
        NativeStore::preflight(self)
    }
    fn logout(&self) -> Result<(), String> {
        run(self.command().arg("logout"), Duration::from_secs(45)).map(|_| ())
    }
    fn isolated(&self, dir: &Path) -> Result<Box<dyn IsolatedSignIn>, String> {
        Ok(Box::new(Self::isolated(dir)?))
    }
}
impl IsolatedSignIn for NativeStore {
    fn sign_in(&self) -> Command {
        let mut command = self.command();
        command.arg("login");
        command
    }
    fn first_usage(
        &self,
        dir: &Path,
        _login: &Login,
        identity: &Identity,
    ) -> Option<ProviderLimitsDto> {
        crate::limits::login::read(dir, identity, None)
    }
    fn clean(&self) -> Result<(), String> {
        Ok(())
    }
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

#[cfg(test)]
mod tests;
