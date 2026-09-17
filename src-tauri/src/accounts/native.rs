//! Narrow native-store access. No whole-home restores and no provider endpoint/config rewriting.
use super::{
    model::{self, Identity},
    store::Login,
    transaction::{Native, NativeGuard},
    vault,
};
use crate::{
    dto::AgentId,
    process::{wait_with_deadline, CommandOutcome},
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

pub struct NativeStore {
    pub provider: AgentId,
    pub config_home: PathBuf,
    pub config_file: PathBuf,
    pub custom: bool,
    pub use_keychain: bool,
}
enum Target {
    File(PathBuf),
    Keyring { service: String, account: String },
}
impl Target {
    fn read(&self) -> Result<Option<Value>, String> {
        let bytes = match self {
            Self::File(path) => match fs::read(path) {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err("Cannot read native credentials.".into()),
            },
            Self::Keyring { service, account } => {
                #[cfg(target_os = "macos")]
                {
                    match crate::limits::credentials::keychain_json(service, Some(account))? {
                        Some(value) => value.into_bytes(),
                        None => return Ok(None),
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    match keyring::Entry::new(service, account)
                        .map_err(|_| "Cannot open native credential store.")?
                        .get_secret()
                    {
                        Ok(v) => v,
                        Err(keyring::Error::NoEntry) => return Ok(None),
                        Err(_) => {
                            return Err("Native credential access was denied or unavailable.".into())
                        }
                    }
                }
            }
        };
        serde_json::from_slice(&bytes).map(Some).map_err(|_| {
            "The native credential document is malformed. It has not been changed.".into()
        })
    }
    fn write(&self, value: Option<&Value>) -> Result<(), String> {
        let bytes = value
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| "Cannot encode native credentials.")?;
        match self {
            Self::File(path) => {
                if let Some(bytes) = bytes {
                    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
                        return Err("Refusing to replace a linked credential file.".into());
                    }
                    vault::atomic_write(path, &bytes)
                } else {
                    match fs::remove_file(path) {
                        Ok(()) => Ok(()),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(_) => Err("Cannot remove native credentials.".into()),
                    }
                }
            }
            Self::Keyring { service, account } => {
                let entry = keyring::Entry::new(service, account)
                    .map_err(|_| "Cannot open native credential store.")?;
                if let Some(bytes) = bytes {
                    entry
                        .set_secret(&bytes)
                        .map_err(|_| "Cannot update native credential store.".into())
                } else {
                    match entry.delete_credential() {
                        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                        Err(_) => Err("Cannot remove native credentials.".into()),
                    }
                }
            }
        }
    }
}
impl NativeStore {
    pub fn resolve(provider: AgentId, home: &Path) -> Result<Self, String> {
        let (env, folder) = match provider {
            AgentId::Codex => ("CODEX_HOME", ".codex"),
            AgentId::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
            _ => return Err("Profiles are unsupported for this provider.".into()),
        };
        // ON_N_OFF_HOME always isolates tests and development from real native homes.
        let override_home = if std::env::var_os("ON_N_OFF_HOME").is_none() {
            std::env::var_os(env)
        } else {
            None
        };
        let custom = override_home.is_some();
        let config_home = override_home
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(folder));
        if !config_home.is_absolute() {
            return Err("The provider home must be an absolute path.".into());
        }
        let config_file = if provider == AgentId::Claude {
            let legacy = config_home.join(".config.json");
            if legacy.exists() {
                legacy
            } else if custom {
                config_home.join(".claude.json")
            } else {
                home.join(".claude.json")
            }
        } else {
            config_home.join("config.toml")
        };
        Ok(Self {
            provider,
            config_home,
            config_file,
            custom,
            use_keychain: std::env::var_os("ON_N_OFF_HOME").is_none(),
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
        store.config_file = store.config_home.join(if provider == AgentId::Codex {
            "config.toml"
        } else {
            ".claude.json"
        });
        fs::create_dir_all(&store.config_home).map_err(|_| "Cannot create isolated login home.")?;
        Ok(store)
    }
    pub fn preflight(&self) -> Result<(), String> {
        if self.custom {
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
            let config = read_toml(&self.config_file)?;
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
    fn target(&self) -> Result<Target, String> {
        if self.provider == AgentId::Codex {
            let config = read_toml(&self.config_file)?;
            if config.get("profile").is_some() {
                return Err("Codex configuration profiles must use the official account controls until their effective credential backend can be verified.".into());
            }
            let mode = config
                .get("cli_auth_credentials_store")
                .and_then(toml::Value::as_str)
                .unwrap_or("file");
            let file = Target::File(self.config_home.join("auth.json"));
            match mode {
    "file"=>Ok(file),
    "keyring"|"auto"=>{
     let canonical=fs::canonicalize(&self.config_home).map_err(|_|"Cannot resolve native Codex home.")?;
     let hash=hash_path(&canonical);let keyring=Target::Keyring{service:"Codex Auth".into(),account:format!("cli|{}",&hash[..16])};
     if mode=="auto"&&keyring.read()?.is_none(){Ok(file)}else{Ok(keyring)}
    },
    _=>Err("This Codex credential backend cannot be activated by on-n-off. Use official sign-in.".into())
   }
        } else {
            if self.use_keychain {
                #[cfg(target_os = "macos")]
                {
                    // A disposable ON_N_OFF_HOME uses file fixtures unless it is an explicit isolated login.
                    let service = self.claude_service();
                    if let Some(account) = keychain_account(&service)? {
                        return Ok(Target::Keyring { service, account });
                    }
                }
            }
            Ok(Target::File(self.config_home.join(".credentials.json")))
        }
    }
    #[cfg(target_os = "macos")]
    fn claude_service(&self) -> String {
        if self.custom {
            format!(
                "Claude Code-credentials-{}",
                &crate::sha::sha256_hex(
                    unicode_normalization::UnicodeNormalization::nfc(
                        self.config_home.to_string_lossy().as_ref()
                    )
                    .collect::<String>()
                    .as_bytes()
                )[..8]
            )
        } else {
            "Claude Code-credentials".into()
        }
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
        command.current_dir(&self.config_home);
        command
    }
    fn verify_claude(&self, profile_url: &str) -> Result<(), String> {
        if !self.custom {
            let home = self
                .config_home
                .parent()
                .ok_or("Cannot resolve native account home.")?;
            let probe = || {
                if self.use_keychain {
                    crate::limits::credentials::keychain_claude_json()
                } else {
                    Ok(None)
                }
            };
            let lookup = super::claude_renew::current_login(
                home,
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
            if let Some(account) = keychain_account(&service)? {
                Target::Keyring { service, account }.write(None)?;
            }
        }
        Ok(())
    }
}
impl Native for NativeStore {
    fn lock(&self) -> Result<Box<dyn NativeGuard>, String> {
        NativeLocks::acquire(self).map(|guard| Box::new(guard) as Box<dyn NativeGuard>)
    }
    fn read(&self) -> Result<Option<Login>, String> {
        let Some(auth) = self.target()?.read()? else {
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
        if self.provider == AgentId::Claude && auth.get("claudeAiOauth").is_none() {
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
        model::identity(self.provider, &login.auth, &login.account)
    }
    fn write(&self, login: Option<&Login>) -> Result<(), String> {
        let _locks = self.lock()?;
        self.write_locked(login, _locks.as_ref())
    }
    fn write_locked(&self, login: Option<&Login>, locks: &dyn NativeGuard) -> Result<(), String> {
        locks.ensure()?;
        let target = self.target()?;
        if self.provider == AgentId::Codex {
            return target.write(login.map(|l| &l.auth));
        }
        let mut auth = target.read()?.unwrap_or_else(|| json!({}));
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
        locks.ensure()?;
        crate::config_io::ConfigIo::patch_account_identity(
            &self.config_file,
            login.map(|l| &l.account),
        )?;
        locks.ensure()?;
        target.write(Some(&auth))
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
        self.provider == AgentId::Codex && model::codex_renews_soon(&login.auth, now)
    }
    fn verify_observed(&self) -> Result<(), String> {
        if self.provider == AgentId::Codex && !self.custom {
            self.verify_codex(false)
        } else {
            self.verify()
        }
    }
}
impl NativeStore {
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
fn read_toml(path: &Path) -> Result<toml::Value, String> {
    match fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).map_err(|_| "Native configuration is malformed.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(toml::Value::Table(Default::default()))
        }
        Err(_) => Err("Cannot read native configuration.".into()),
    }
}
fn hash_path(path: &Path) -> String {
    crate::sha::sha256_hex(path.to_string_lossy().as_bytes())
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
#[cfg(target_os = "macos")]
fn keychain_account(service: &str) -> Result<Option<String>, String> {
    let child = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "Cannot inspect native Keychain entry.")?;
    match wait_with_deadline(child, Duration::from_secs(10)) {
        Ok(CommandOutcome::Exited {
            success: true,
            stdout,
            ..
        }) => stdout
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("\"acct\"<blob>=\"")
                    .and_then(|s| s.strip_suffix('"'))
                    .map(str::to_owned)
            })
            .map(Some)
            .ok_or_else(|| "Cannot identify the native Keychain entry.".into()),
        Ok(CommandOutcome::Exited { stderr, .. }) if stderr.contains("could not be found") => {
            Ok(None)
        }
        _ => Err("Native Keychain access was denied or unavailable.".into()),
    }
}
struct NativeLocks {
    paths: Vec<PathBuf>,
    stop: Option<std::sync::mpsc::Sender<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
    lost: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl NativeLocks {
    fn acquire(store: &NativeStore) -> Result<Self, String> {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let mut lock = Self {
            paths: Vec::new(),
            stop: None,
            worker: None,
            lost: Arc::new(AtomicBool::new(false)),
        };
        if store.provider != AgentId::Claude {
            return Ok(lock);
        }
        let mut legacy = store.config_home.as_os_str().to_owned();
        legacy.push(".lock");
        let mut config = store.config_file.as_os_str().to_owned();
        config.push(".lock");
        for (path, stale_seconds) in [
            (store.config_home.join(".oauth_refresh.lock"), 60),
            (PathBuf::from(legacy), 60),
            (PathBuf::from(config), 10),
        ] {
            if fs::create_dir(&path).is_err() {
                let stale = fs::symlink_metadata(&path)
                    .ok()
                    .filter(|m| m.is_dir() && !m.file_type().is_symlink())
                    .and_then(|m| m.modified().ok())
                    .and_then(|at| at.elapsed().ok())
                    .is_some_and(|age| age > Duration::from_secs(stale_seconds));
                if !stale {
                    return Err(
                        "Claude is updating its login or configuration. Retry after it finishes."
                            .into(),
                    );
                }
                fs::remove_dir(&path)
                    .and_then(|()| fs::create_dir(&path))
                    .map_err(|_| {
                        "Claude is updating its login or configuration. Retry after it finishes."
                    })?;
            }
            lock.paths.push(path);
        }
        let (stop, receive) = std::sync::mpsc::channel();
        let paths = lock.paths.clone();
        let lost = lock.lost.clone();
        lock.stop = Some(stop);
        lock.worker = Some(std::thread::spawn(move || {
            while receive.recv_timeout(Duration::from_secs(2))
                == Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            {
                for path in &paths {
                    if filetime::set_file_mtime(path, filetime::FileTime::now()).is_err() {
                        lost.store(true, Ordering::Release);
                        return;
                    }
                }
            }
        }));
        Ok(lock)
    }
}
impl NativeGuard for NativeLocks {
    fn ensure(&self) -> Result<(), String> {
        if self.lost.load(std::sync::atomic::Ordering::Acquire) {
            Err(
                "Native credential coordination was lost. Protected recovery has been retained."
                    .into(),
            )
        } else {
            Ok(())
        }
    }
}
impl Drop for NativeLocks {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        for path in self.paths.iter().rev() {
            let _ = fs::remove_dir(path);
        }
    }
}

/// Metadata projection from the backend selected by native Codex config. No credential leaves accounts.
pub(crate) fn codex_metadata(config_home: &Path) -> Result<Option<(String, Value)>, String> {
    let native = NativeStore {
        provider: AgentId::Codex,
        config_home: config_home.into(),
        config_file: config_home.join("config.toml"),
        custom: true,
        use_keychain: true,
    };
    let Some(login) = native.read()? else {
        return Ok(None);
    };
    let Some(workspace) = login
        .auth
        .pointer("/tokens/account_id")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    else {
        return Ok(None);
    };
    let claims = if login.auth.pointer("/tokens/id_token").is_some() {
        let payload = model::claims(&login.auth)?;
        let claims = payload
            .get("https://api.openai.com/auth")
            .cloned()
            .ok_or("Missing native account claims.")?;
        if claims.get("chatgpt_account_id").and_then(Value::as_str) != Some(workspace) {
            return Err("Native workspace claims disagree.".into());
        }
        claims
    } else {
        json!({"chatgpt_account_id":workspace})
    };
    Ok(Some((
        model::codex_observation_key(workspace, &claims),
        claims,
    )))
}

#[cfg(test)]
mod tests;
