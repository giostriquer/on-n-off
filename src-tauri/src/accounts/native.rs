//! Narrow native-store access. No whole-home restores and no provider endpoint/config rewriting.
use super::{
    claude_store::{
        self, BeginError, ClaudeLocks, KeychainProbe, LockError, LockScope, SecureStorage,
        StorageDir, StoreError, Stored,
    },
    codex_store,
    model::{self, Identity},
    store::Login,
    transaction::{Native, NativeGuard, ReadBack},
};
use crate::{
    dto::AgentId,
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

/// Claude Code holds one of the locks an account change needs.
const BUSY: &str = "Claude is updating its login or configuration. Retry after it finishes.";

/// A lock an account change relied on was taken away while it was held.
const LOST: &str = "Native credential coordination was lost. Protected recovery has been retained.";

/// How the account switch words a store it could not read.
fn store_error(error: StoreError) -> String {
    match error {
        StoreError::Keychain(why) => why,
        StoreError::FileUnreadable(_) => "Cannot read native credentials.".into(),
        StoreError::FileMalformed(_) => {
            "The native credential document is malformed. It has not been changed.".into()
        }
    }
}

pub struct NativeStore {
    pub provider: AgentId,
    pub config_home: PathBuf,
    pub config_file: PathBuf,
    pub custom: bool,
    pub use_keychain: bool,
    /// Where `CLAUDE_SECURESTORAGE_CONFIG_DIR` moved Claude's login and locks; `None` keeps them
    /// in the config home, and keeps the variable away from a `claude` this store starts.
    pub secure_storage: Option<SecureStorage>,
}
impl NativeStore {
    pub fn resolve(provider: AgentId, home: &Path) -> Result<Self, String> {
        Self::resolve_from(provider, home, &crate::paths::process_env)
    }
    fn resolve_from(
        provider: AgentId,
        home: &Path,
        lookup: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<Self, String> {
        // ON_N_OFF_HOME always isolates tests and development from real native homes.
        let disposable = lookup("ON_N_OFF_HOME").is_some();
        let mut secure_storage = None;
        let mut claude_config_file = None;
        let (config_home, custom) = match provider {
            AgentId::Codex => {
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
                (config_home, custom)
            }
            AgentId::Claude => {
                let dirs = claude_store::dirs(home, lookup)?;
                claude_config_file = Some(dirs.config_file(home));
                secure_storage = dirs.secure_storage;
                (dirs.config, dirs.custom)
            }
            _ => return Err("Profiles are unsupported for this provider.".into()),
        };
        let config_file = claude_config_file.unwrap_or_else(|| config_home.join("config.toml"));
        Ok(Self {
            provider,
            config_home,
            config_file,
            custom,
            use_keychain: !disposable,
            secure_storage,
        })
    }
    pub fn isolated(provider: AgentId, home: &Path) -> Result<Self, String> {
        let mut store = Self::resolve(provider, home)?;
        store.config_home = home.join(if provider == AgentId::Codex {
            ".codex"
        } else {
            ".claude"
        });
        store.custom = true;
        store.use_keychain = true;
        store.secure_storage = None;
        store.config_file = store.config_home.join(if provider == AgentId::Codex {
            "config.toml"
        } else {
            ".claude.json"
        });
        fs::create_dir_all(&store.config_home).map_err(|_| "Cannot create isolated login home.")?;
        Ok(store)
    }
    pub fn preflight(&self) -> Result<(), String> {
        // A store `CLAUDE_SECURESTORAGE_CONFIG_DIR` moved is as custom as a home the provider's
        // own variable chose: account changes defer to the official client for both.
        if self.custom || self.storage_moved() {
            return Err("Account activation currently supports the default CLI home. Remove the custom home override or use the official CLI for this context.".into());
        }

        if fs::symlink_metadata(&self.config_file).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(
                "Linked account configuration must be changed through the official CLI.".into(),
            );
        }
        let overrides = if self.provider == AgentId::Codex {
            ["OPENAI_API_KEY", "CODEX_API_KEY", "CODEX_AUTH_TOKEN"]
        } else {
            [
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "CLAUDE_CODE_OAUTH_TOKEN",
            ]
        };
        for name in overrides {
            if std::env::var_os(name).is_some() {
                return Err("An environment credential overrides native login. Remove the override before using saved profiles.".into());
            }
        }
        if self.provider == AgentId::Codex {
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
                    "Managed Codex authentication must be changed through the official client."
                        .into(),
                );
            }
        } else {
            for path in [
                self.config_home.join("settings.json"),
                PathBuf::from("/Library/Application Support/ClaudeCode/managed-settings.json"),
            ] {
                let settings = read_json(&path)?;
                if settings.get("forceLoginMethod").is_some()
                    || settings.get("forceLoginOrgUUID").is_some()
                {
                    return Err("Managed Claude authentication must be changed through the official client.".into());
                }
            }
        }
        Ok(())
    }
    /// Claude Code's storage dir for this store.
    pub(crate) fn claude_dir(&self) -> StorageDir {
        StorageDir::of(&self.config_home, self.custom, self.secure_storage.as_ref())
    }
    /// `CLAUDE_SECURESTORAGE_CONFIG_DIR` keeps Claude's login somewhere other than the config home
    /// alone would: another dir, or the same one under a scoped Keychain entry.
    fn storage_moved(&self) -> bool {
        self.claude_dir() != StorageDir::new(self.config_home.clone(), self.custom)
    }
    /// Claude's login, from the store Claude Code would read it from. A disposable ON_N_OFF_HOME
    /// uses file fixtures unless it is an explicit isolated login.
    fn claude_read(&self) -> Result<Stored, String> {
        claude_store::read(&self.claude_dir(), self.claude_keychain()).map_err(store_error)
    }
    /// The Keychain entry's secret, found the way Claude Code finds it.
    fn claude_keychain(&self) -> KeychainProbe {
        #[cfg(target_os = "macos")]
        if self.use_keychain {
            return claude_store::keychain_secret(&self.claude_service());
        }
        Ok(None)
    }
    #[cfg(target_os = "macos")]
    fn claude_service(&self) -> String {
        self.claude_dir().service()
    }
    pub fn command(&self) -> Command {
        let name = if self.provider == AgentId::Codex {
            "codex"
        } else {
            "claude"
        };
        let binary = crate::cli_locate::resolve_provider_cli(self.provider, name)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.into());
        let mut command = crate::cli::AgentCli::new(binary).command();
        if self.provider == AgentId::Codex {
            command.env("CODEX_HOME", &self.config_home);
        } else if self.custom || !self.use_keychain {
            command.env("CLAUDE_CONFIG_DIR", &self.config_home);
            // macOS locates the login Keychain through HOME. Redirect only Claude's config
            // for real isolated sign-ins; file-backed fixtures still need a disposable OS home.
            if let Some(home) = self
                .config_home
                .parent()
                .filter(|_| !cfg!(target_os = "macos") || !self.use_keychain)
            {
                command.env("HOME", home);
                command.env("USERPROFILE", home);
            }
        } else {
            command.env_remove("CLAUDE_CONFIG_DIR");
        }
        if self.provider == AgentId::Claude {
            // The child works in the store this one resolved, never one an inherited variable
            // chose: an isolated sign-in would otherwise land in the user's own store.
            match &self.secure_storage {
                Some(secure) => command.env(claude_store::SECURE_STORAGE_VAR, &secure.var),
                None => command.env_remove(claude_store::SECURE_STORAGE_VAR),
            };
        }
        command.current_dir(&self.config_home);
        command
    }
    fn verify_claude(&self, profile_url: &str) -> Result<(), String> {
        if !self.custom {
            let probe = |_: &StorageDir| self.claude_keychain();
            let lookup = super::claude_renew::current_login(
                &self.claude_dir(),
                &probe,
                chrono::Utc::now().timestamp_millis(),
                super::claude_renew::TOKEN_URL,
            );
            if !matches!(
                lookup,
                crate::limits::credentials::CredentialLookup::Found(_)
            ) {
                return Err(
                    "Could not renew the native Claude login. Sign in again if it has expired."
                        .into(),
                );
            }
        }
        let login = self.read()?.ok_or("No native login was found.")?;
        let identity = self.identify(&login)?;
        let token = model::string(&login.auth, "/claudeAiOauth/accessToken")?;
        let profile = crate::http::get_json(
            profile_url,
            &[
                ("Authorization", &format!("Bearer {token}")),
                ("anthropic-beta", "oauth-2025-04-20"),
            ],
        )
        .map_err(|_| "Could not verify the Claude login. Check connectivity or sign in again.")?;
        if profile.pointer("/account/uuid").and_then(Value::as_str) != Some(&identity.user_id)
            || profile
                .pointer("/organization/uuid")
                .and_then(Value::as_str)
                != Some(&identity.workspace_id)
        {
            return Err("Claude credential and organization identity disagree.".into());
        }
        let current = self
            .read()?
            .ok_or("Native login disappeared during verification.")?;
        if self.identify(&current)? != identity || current.auth != login.auth {
            return Err(
                "Native login changed during verification. Retry the account operation.".into(),
            );
        }
        Ok(())
    }
    pub fn logout(&self) -> Result<(), String> {
        let mut command = self.command();
        if self.provider == AgentId::Claude {
            command.arg("auth");
        }
        command.arg("logout");
        run(&mut command, Duration::from_secs(45)).map(|_| ())
    }
    pub fn clean_isolated(&self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        if self.provider == AgentId::Claude {
            let service = self.claude_service();
            if let Some(account) = claude_store::keychain_account(&service)? {
                super::keychain::delete(&service, &account)?;
            }
        }
        Ok(())
    }
}
impl Native for NativeStore {
    fn lock(&self) -> Result<Box<dyn NativeGuard>, String> {
        if self.provider != AgentId::Claude {
            return Ok(Box::new(()));
        }
        ClaudeLocks::acquire(
            &self.claude_dir(),
            LockScope::RefreshAndConfig(&self.config_file),
        )
        .map(|guard| Box::new(guard) as Box<dyn NativeGuard>)
        .map_err(|error| match error {
            LockError::Busy => BUSY.into(),
            LockError::Unavailable(why) => format!("Cannot take Claude Code's locks: {why}"),
        })
    }
    fn read(&self) -> Result<Option<Login>, String> {
        let auth = if self.provider == AgentId::Claude {
            self.claude_read()?.document
        } else {
            codex_store::target(&self.config_home)?.read()?
        };
        let Some(auth) = auth else {
            return Ok(None);
        };
        let account = if self.provider == AgentId::Claude {
            read_json(&self.config_file)?
                .get("oauthAccount")
                .cloned()
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        // Claude Code signs out by emptying `claudeAiOauth`, so a login needs an access token, by
        // the same rule the Limits read applies.
        if self.provider == AgentId::Claude
            && crate::limits::credentials::parse_claude_credential(&auth).is_none()
        {
            return Ok(None);
        }
        let auth = if self.provider == AgentId::Claude {
            json!({"claudeAiOauth":auth["claudeAiOauth"]})
        } else {
            auth
        };
        Ok(Some(Login { auth, account }))
    }
    fn identify(&self, login: &Login) -> Result<Identity, String> {
        super::view(self.provider, login)?.identity()
    }
    fn write(&self, login: Option<&Login>) -> Result<(), String> {
        self.write_locked(login, self.lock()?).map(drop)
    }
    fn write_locked(
        &self,
        login: Option<&Login>,
        locks: Box<dyn NativeGuard>,
    ) -> Result<ReadBack, String> {
        self.publish(login, locks.as_ref())?;
        let back = self.read();
        drop(locks);
        Ok(back)
    }
    fn verify(&self) -> Result<(), String> {
        if self.provider == AgentId::Claude {
            return self.verify_claude("https://api.anthropic.com/api/oauth/profile");
        }
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
        self.provider == AgentId::Codex && super::codex::CodexLogin::of(login).renews_soon(now)
    }
    fn verify_observed(&self) -> Result<(), String> {
        if self.provider == AgentId::Codex && !self.custom {
            self.verify_codex(false)
        } else {
            self.verify()
        }
    }
}
impl super::NativeAccount for NativeStore {
    fn preflight(&self) -> Result<(), String> {
        NativeStore::preflight(self)
    }
    fn logout(&self) -> Result<(), String> {
        NativeStore::logout(self)
    }
}
impl NativeStore {
    /// The write itself, under `locks`: Codex's store as its config selects, or Claude's merged
    /// into the store Claude Code reads, beside the identity `ConfigIo` patches.
    fn publish(&self, login: Option<&Login>, locks: &dyn NativeGuard) -> Result<(), String> {
        locks.ensure()?;
        if self.provider == AgentId::Codex {
            return codex_store::target(&self.config_home)?.write(login.map(|l| &l.auth));
        }
        // The read, the config patch and the credential write are one change under Claude Code's
        // storage-write lock, going to the store Claude Code's next read uses.
        let held = || locks.ensure().is_err();
        let keychain = |_: &StorageDir| self.claude_keychain();
        let begin_error = |error| match error {
            BeginError::Busy => BUSY.to_string(),
            BeginError::Lock(why) => format!("Cannot take Claude Code's storage lock: {why}"),
            BeginError::Store(error) => store_error(error),
            BeginError::Unavailable(why) => why,
        };
        let (document, pending) =
            claude_store::begin(&self.claude_dir(), &keychain, &held).map_err(begin_error)?;
        let write = pending.prove().map_err(begin_error)?;
        let mut auth = document.unwrap_or_else(|| json!({}));
        let object = auth
            .as_object_mut()
            .ok_or("Malformed Claude credentials.")?;
        if let Some(login) = login {
            object.insert(
                "claudeAiOauth".into(),
                login
                    .auth
                    .get("claudeAiOauth")
                    .cloned()
                    .ok_or("Missing Claude login.")?,
            );
        } else {
            object.remove("claudeAiOauth");
        }
        // The caller's encrypted journal already holds the outgoing OAuth identity and credential.
        // ConfigIo owns atomic config publication/validation/rollback; ordinary backups get no tokens.
        let ensure = || {
            if write.lost() {
                Err(LOST.to_string())
            } else {
                Ok(())
            }
        };
        ensure()?;
        crate::config_io::ConfigIo::patch_account_identity(
            &self.config_file,
            login.map(|l| &l.account),
        )?;
        ensure()?;
        write.commit(&auth)
    }
    fn verify_codex(&self, force: bool) -> Result<(), String> {
        let entries = crate::limits::read_limits(self.provider, force);
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
impl NativeGuard for ClaudeLocks {
    fn ensure(&self) -> Result<(), String> {
        if self.lost() {
            Err(LOST.into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
