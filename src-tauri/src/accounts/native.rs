//! Narrow native-store access. No whole-home restores and no provider endpoint/config rewriting.
use super::{
    claude_store::{
        self, ClaudeLocks, ClaudeStore, ConfigDir, KeychainProbe, LockScope, StoreError, Stored,
    },
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
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

/// Claude Code holds one of the locks an account change needs.
const BUSY: &str = "Claude is updating its login or configuration. Retry after it finishes.";

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
                    match super::keychain::find_password(service, Some(account))? {
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
                // Through `security`, the identity the item already trusts for reads, never this
                // ad-hoc-signed process: `keychain.rs` says what the latter cost.
                #[cfg(target_os = "macos")]
                {
                    match bytes {
                        Some(bytes) => super::keychain::write(service, account, &bytes),
                        None => super::keychain::delete(service, account),
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
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
}
/// The environment [`NativeStore::resolve`] reads: the process's own. A test binary sees only a
/// disposable `ON_N_OFF_HOME` instead, so no test can follow a developer's `CLAUDE_CONFIG_DIR` or
/// `CODEX_HOME` to a real home, or choose the login Keychain; a test that needs another
/// environment hands it to `resolve_from`.
#[cfg(not(test))]
fn process_env(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}
#[cfg(test)]
fn process_env(name: &str) -> Option<OsString> {
    (name == "ON_N_OFF_HOME").then(|| OsString::from("disposable"))
}

impl NativeStore {
    pub fn resolve(provider: AgentId, home: &Path) -> Result<Self, String> {
        Self::resolve_from(provider, home, &process_env)
    }
    fn resolve_from(
        provider: AgentId,
        home: &Path,
        lookup: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<Self, String> {
        let (variable, folder) = match provider {
            AgentId::Codex => ("CODEX_HOME", ".codex"),
            AgentId::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
            _ => return Err("Profiles are unsupported for this provider.".into()),
        };
        // ON_N_OFF_HOME always isolates tests and development from real native homes.
        let disposable = lookup("ON_N_OFF_HOME").is_some();
        let override_home = if disposable { None } else { lookup(variable) };
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
            use_keychain: !disposable,
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
    /// Codex's store, from the backend its config selects. Claude's is `claude_store`'s question.
    fn codex_target(&self) -> Result<Target, String> {
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
    }
    /// Claude Code's config dir for this store.
    fn claude_dir(&self) -> ConfigDir {
        ConfigDir::new(self.config_home.clone())
    }
    /// Claude's login, from the store Claude Code would read it from. A disposable ON_N_OFF_HOME
    /// uses file fixtures unless it is an explicit isolated login.
    fn claude_read(&self) -> Result<Stored, String> {
        claude_store::read(&self.claude_dir(), self.claude_keychain()).map_err(
            |error| match error {
                StoreError::Keychain(why) => why,
                StoreError::FileUnreadable(_) => "Cannot read native credentials.".into(),
                StoreError::FileMalformed(_) => {
                    "The native credential document is malformed. It has not been changed.".into()
                }
            },
        )
    }
    /// The Keychain entry's secret, found the way Claude Code finds it.
    fn claude_keychain(&self) -> KeychainProbe {
        #[cfg(target_os = "macos")]
        if self.use_keychain {
            return claude_store::keychain_secret(&self.claude_service());
        }
        Ok(None)
    }
    /// The write that reaches `store`, resolved before anything is written.
    fn claude_target(&self, store: ClaudeStore) -> Result<Target, String> {
        match store {
            ClaudeStore::File(path) => Ok(Target::File(path)),
            #[cfg(target_os = "macos")]
            ClaudeStore::Keychain => {
                let service = self.claude_service();
                let account = claude_store::keychain_account(&service)?
                    .ok_or("The native Keychain entry disappeared.")?;
                Ok(Target::Keyring { service, account })
            }
            #[cfg(not(target_os = "macos"))]
            ClaudeStore::Keychain => Err("This platform has no Keychain.".into()),
        }
    }
    #[cfg(target_os = "macos")]
    fn claude_service(&self) -> String {
        if self.custom {
            self.claude_dir().scoped_service()
        } else {
            claude_store::CLAUDE_KEYCHAIN_SERVICE.into()
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
                    claude_store::keychain_probe()
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
            if let Some(account) = claude_store::keychain_account(&service)? {
                Target::Keyring { service, account }.write(None)?;
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
        .map_err(|_| BUSY.into())
    }
    fn read(&self) -> Result<Option<Login>, String> {
        let auth = if self.provider == AgentId::Claude {
            self.claude_read()?.document
        } else {
            self.codex_target()?.read()?
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
        if self.provider == AgentId::Codex {
            return self.codex_target()?.write(login.map(|l| &l.auth));
        }
        // Claude Code changes its credentials only under this lock, so the read, the config patch
        // and the write below cannot interleave with one of its own.
        let _storage = ClaudeLocks::acquire(&self.claude_dir(), LockScope::StorageWrite)
            .map_err(|_| BUSY.to_string())?;
        let stored = self.claude_read()?;
        // Written to the store Claude Code's next read uses; when the Keychain could not be read,
        // that store is unknown and nothing is written at all.
        let target = self.claude_target(stored.target?)?;
        let mut auth = stored.document.unwrap_or_else(|| json!({}));
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
impl NativeGuard for ClaudeLocks {
    fn ensure(&self) -> Result<(), String> {
        if self.lost() {
            Err(
                "Native credential coordination was lost. Protected recovery has been retained."
                    .into(),
            )
        } else {
            Ok(())
        }
    }
}

/// Metadata projection from the backend selected by native Codex config. No credential leaves
/// accounts here; `codex_metadata_and_access` is the one projection that carries the access token.
pub(crate) fn codex_metadata(config_home: &Path) -> Result<Option<(String, Value)>, String> {
    match codex_login(config_home)? {
        Some(login) => codex_identity(&login),
        None => Ok(None),
    }
}

/// The native Codex login from the backend its config selects.
fn codex_login(config_home: &Path) -> Result<Option<Login>, String> {
    NativeStore {
        provider: AgentId::Codex,
        config_home: config_home.into(),
        config_file: config_home.join("config.toml"),
        custom: true,
        use_keychain: true,
    }
    .read()
}

/// A Codex login's observation key and workspace claims, refusing claims for another workspace.
fn codex_identity(login: &Login) -> Result<Option<(String, Value)>, String> {
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

/// The signed-in Codex login's access token, beside the identity it belongs to, for the two requests
/// on-n-off makes with that login itself: a workspace member's spending (`limits/credits_spent.rs`)
/// and the subscription's term (`limits/renewal.rs`), read-only GETs the user chose to allow on
/// 2026-09-24 and 2026-09-25. Only the access token leaves accounts.
pub(crate) struct CodexAccess {
    /// The same key `codex_metadata` gives, so the caller can match the token to a card.
    pub observation_key: String,
    /// The `ChatGPT-Account-Id` the request is made for.
    pub workspace_id: String,
    pub token: model::AccessToken,
}

/// A metadata projection: the observation key and the workspace claims (`codex_metadata`).
pub(crate) type CodexMetadata = (String, Value);

/// `codex_metadata`, and the login's access projection when it holds an access token, from one
/// read of the native store: the signed-in read's identity check after the app-server handshake
/// takes it, so the backend reads cost no read of their own. `None` for no
/// login; the access is `None` for a login without an access token.
pub(crate) fn codex_metadata_and_access(
    config_home: &Path,
) -> Result<Option<(CodexMetadata, Option<CodexAccess>)>, String> {
    let Some(login) = codex_login(config_home)? else {
        return Ok(None);
    };
    let Some((observation_key, claims)) = codex_identity(&login)? else {
        return Ok(None);
    };
    let access = match model::string(&login.auth, "/tokens/access_token") {
        Ok(token) => Some(CodexAccess {
            observation_key: observation_key.clone(),
            workspace_id: claims
                .get("chatgpt_account_id")
                .and_then(Value::as_str)
                .ok_or("Missing native account claims.")?
                .to_string(),
            token: model::AccessToken::new(token),
        }),
        Err(_) => None,
    };
    Ok(Some(((observation_key, claims), access)))
}

#[cfg(test)]
mod tests;
