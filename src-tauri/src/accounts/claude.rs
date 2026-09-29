//! The Claude adapter: Claude's half of the accounts seam. It reads a Claude login by Claude
//! Code's rules ([`ClaudeLogin`]): `claudeAiOauth` as Claude Code stores it, beside the
//! `oauthAccount` record that says who it is. And it is the account switch's view of Claude Code's
//! native store ([`ClaudeNative`]): which one the environment selects and when account changes
//! defer to the official client, the `claude` it starts and its environment, the locks, the
//! read, the write and its verification, and an isolated sign-in. Where the login lives and how it
//! is locked is `claude_store`'s question; this adapter asks it.
use super::{
    claude_store::{
        self, BeginError, ClaudeLocks, KeychainProbe, LockError, LockScope, SecureStorage,
        StorageDir, StoreError, Stored,
    },
    clients::Client,
    model::{self, Identity, LoginView},
    native::{self, CUSTOM_HOME},
    store::Login,
    transaction::{Native, NativeGuard, ReadBack},
    Home, IsolatedSignIn, NativeAccount,
};
use crate::dto::{AgentId, ProviderLimitsDto};
use serde_json::{json, Value};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

mod home;

pub(super) struct Claude;

impl super::Adapter for Claude {
    fn native(&self, home: &Path) -> Result<Box<dyn NativeAccount>, String> {
        Ok(Box::new(ClaudeNative::resolve(home)?))
    }

    fn isolated(&self, dir: &Path) -> Result<Box<dyn IsolatedSignIn>, String> {
        Ok(Box::new(ClaudeNative::isolated(dir)?))
    }

    fn login<'a>(&self, login: &'a Login) -> Box<dyn LoginView + 'a> {
        Box::new(ClaudeLogin::of(login))
    }

    fn token_url(&self) -> Option<&'static str> {
        None
    }

    /// Never sent: a saved Claude login waits in its home, where Claude Code renews it.
    fn renew_private(&self, _: &Login, _: i64, _: &str) -> Result<Login, String> {
        Err("Claude Code renews a saved Claude login itself, in the account's home.".into())
    }

    fn homes(&self) -> Option<super::HomeAt> {
        Some(|dir| Box::new(home::ClaudeHome::at(dir)))
    }

    /// Never sent: a saved Claude login is read only in its home, by Claude Code. One still in the
    /// vault is one the last read could not move there, and the next read tries again.
    fn read_usage(
        &self,
        _: &Identity,
        _: &Login,
        _: i64,
        _: &crate::limits::SavedReadUrls<'_>,
    ) -> Result<ProviderLimitsDto, crate::limits::SavedReadError> {
        Err(crate::limits::SavedReadError::Unavailable(
            "This account's login has not moved into its home yet; the next read tries again.",
        ))
    }

    /// Claude Code handles a native credential change itself, so its clients refuse no switch.
    fn client(&self) -> &'static Client {
        &Client {
            name: "claude",
            package_entry: "/@anthropic-ai/claude-code/cli.js",
            blocks_activation: false,
        }
    }
}

/// Claude Code holds one of the locks an account change needs.
const BUSY: &str = "Claude is updating its login or configuration. Retry after it finishes.";

/// A lock an account change relied on was taken away while it was held.
const LOST: &str = "Native credential coordination was lost. Protected recovery has been retained.";

/// Where an administrator's managed Claude Code settings live.
const MANAGED_SETTINGS: &str = "/Library/Application Support/ClaudeCode/managed-settings.json";

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

/// Claude Code's native store as the account switch uses it: the user's own, where the
/// environment puts it ([`ClaudeNative::resolve`]), an isolated sign-in's, or the one in a saved
/// account's home.
pub(super) struct ClaudeNative {
    config_home: PathBuf,
    /// The file holding the signed-in account record, `oauthAccount`.
    config_file: PathBuf,
    /// `CLAUDE_CONFIG_DIR` chose the config home, or this is a private store: an isolated
    /// sign-in's or a saved account's home.
    custom: bool,
    /// Whether the login may be in the Keychain; a disposable `ON_N_OFF_HOME` keeps file fixtures.
    use_keychain: bool,
    /// Where `CLAUDE_SECURESTORAGE_CONFIG_DIR` moved Claude's login and locks; `None` keeps them
    /// in the config home, and keeps the variable away from a `claude` this store starts.
    secure_storage: Option<SecureStorage>,
    /// A saved account's home, whose login on-n-off files in the home's own Keychain entry on
    /// macOS.
    in_home: bool,
    /// An isolated sign-in's store or a saved account's home, never the user's own, custom or not.
    private: bool,
}

/// The user's own Claude Code store, as the signed-in account's usage read uses it: a `claude`
/// that works in it and the file naming its account.
pub(crate) struct SignedIn(ClaudeNative);

impl SignedIn {
    /// The user's store under `home`, as this process's environment places it.
    pub(crate) fn resolve(home: &Path) -> Result<Self, String> {
        ClaudeNative::resolve(home).map(Self)
    }

    pub(crate) fn command(&self) -> Command {
        self.0.command()
    }

    /// The file holding the signed-in account record, `oauthAccount`.
    pub(crate) fn config_file(&self) -> &Path {
        &self.0.config_file
    }
}

/// What one write to Claude's store changes.
#[derive(Clone, Copy)]
enum Change<'a> {
    /// The login and the account record beside it; `None` removes both.
    Login(Option<&'a Login>),
    /// The login alone, emptied as Claude Code's own sign-out empties it, the account record kept.
    SignedOut,
}

impl ClaudeNative {
    /// The user's store under `home`, as this process's environment places it.
    pub(super) fn resolve(home: &Path) -> Result<Self, String> {
        Self::resolve_from(home, &crate::paths::process_env)
    }

    fn resolve_from(
        home: &Path,
        lookup: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<Self, String> {
        // ON_N_OFF_HOME always isolates tests and development from real native homes.
        let disposable = lookup("ON_N_OFF_HOME").is_some();
        let dirs = claude_store::dirs(home, lookup)?;
        Ok(Self {
            config_file: dirs.config_file(home),
            config_home: dirs.config,
            custom: dirs.custom,
            use_keychain: !disposable,
            secure_storage: dirs.secure_storage,
            in_home: false,
            private: false,
        })
    }

    /// A private store in `dir` for one isolated sign-in: its own config home and scoped Keychain
    /// entry, never a secure-storage dir the environment chose.
    fn isolated(dir: &Path) -> Result<Self, String> {
        let config_home = dir.join(".claude");
        fs::create_dir_all(&config_home).map_err(|_| "Cannot create isolated login home.")?;
        Ok(Self {
            in_home: false,
            ..Self::private(config_home)
        })
    }

    /// The store of the home kept for a saved account in `dir`, created by its first write.
    fn home(dir: &Path) -> Self {
        Self {
            in_home: true,
            ..Self::private(dir.join(".claude"))
        }
    }

    /// A private store in `config_home`, its own config home under its own scoped Keychain entry.
    fn private(config_home: PathBuf) -> Self {
        Self {
            config_file: config_home.join(".claude.json"),
            config_home,
            custom: true,
            use_keychain: true,
            secure_storage: None,
            in_home: false,
            private: true,
        }
    }

    /// `preflight` against the environment `env` reads and the managed settings file at
    /// `managed_settings`.
    fn preflight_in(
        &self,
        env: &dyn Fn(&str) -> Option<OsString>,
        managed_settings: &Path,
    ) -> Result<(), String> {
        // A store `CLAUDE_SECURESTORAGE_CONFIG_DIR` moved is as custom as a home the provider's
        // own variable chose: account changes defer to the official client for both.
        if self.custom || self.storage_moved() {
            return Err(CUSTOM_HOME.into());
        }
        native::refuse_linked(&self.config_file)?;
        native::refuse_env_credentials(&claude_store::ENV_CREDENTIALS, env)?;
        for path in [
            self.config_home.join("settings.json"),
            managed_settings.to_path_buf(),
        ] {
            let settings = native::read_json(&path)?;
            if settings.get("forceLoginMethod").is_some()
                || settings.get("forceLoginOrgUUID").is_some()
            {
                return Err(
                    "Managed Claude authentication must be changed through the official client."
                        .into(),
                );
            }
        }
        Ok(())
    }

    /// Claude Code's storage dir for this store.
    fn storage_dir(&self) -> StorageDir {
        StorageDir::of(&self.config_home, self.custom, self.secure_storage.as_ref())
    }

    /// `CLAUDE_SECURESTORAGE_CONFIG_DIR` keeps Claude's login somewhere other than the config home
    /// alone would: another dir, or the same one under a scoped Keychain entry.
    fn storage_moved(&self) -> bool {
        self.storage_dir() != StorageDir::new(self.config_home.clone(), self.custom)
    }

    /// Claude's login, from the store Claude Code would read it from. A disposable ON_N_OFF_HOME
    /// uses file fixtures unless it is an explicit isolated login.
    fn stored(&self) -> Result<Stored, String> {
        claude_store::read(&self.storage_dir(), self.keychain()).map_err(store_error)
    }

    /// The Keychain entry's secret, found the way Claude Code finds it.
    fn keychain(&self) -> KeychainProbe {
        #[cfg(target_os = "macos")]
        if self.use_keychain {
            return claude_store::keychain_secret(&self.service());
        }
        Ok(None)
    }

    #[cfg(target_os = "macos")]
    fn service(&self) -> String {
        self.storage_dir().service()
    }

    /// A `claude` for this store: handed its config home when it is not the default one, and its
    /// secure-storage dir exactly as resolved, or none.
    fn command(&self) -> Command {
        let mut command = native::cli(AgentId::Claude, "claude");
        if self.custom || !self.use_keychain {
            command.env("CLAUDE_CONFIG_DIR", &self.config_home);
            // macOS locates the login Keychain through HOME. Redirect only Claude's config
            // for real isolated sign-ins; file-backed fixtures still need a disposable OS home.
            // The user's own config dir, custom or not, keeps the user's own OS home.
            if let Some(home) = self
                .config_home
                .parent()
                .filter(|_| !self.use_keychain || (self.private && !cfg!(target_os = "macos")))
            {
                command.env("HOME", home);
                command.env("USERPROFILE", home);
            }
        } else {
            command.env_remove("CLAUDE_CONFIG_DIR");
        }
        // The child works in the store this one resolved, never one an inherited variable
        // chose: an isolated sign-in would otherwise land in the user's own store.
        match &self.secure_storage {
            Some(secure) => command.env(claude_store::SECURE_STORAGE_VAR, &secure.var),
            None => command.env_remove(claude_store::SECURE_STORAGE_VAR),
        };
        command.current_dir(&self.config_home);
        command
    }

    /// `claude auth logout`.
    fn logout_command(&self) -> Command {
        let mut command = self.command();
        command.args(["auth", "logout"]);
        command
    }

    /// The write itself, under `locks`: Claude's login merged into the store Claude Code reads,
    /// beside the identity `ConfigIo` patches, or the login alone emptied.
    fn publish(&self, change: Change<'_>, locks: &dyn NativeGuard) -> Result<(), String> {
        locks.ensure()?;
        // The read, the config patch and the credential write are one change under Claude Code's
        // storage-write lock, going to the store Claude Code's next read uses.
        let held = || locks.ensure().is_err();
        let keychain = |_: &StorageDir| self.keychain();
        let begin_error = |error| match error {
            BeginError::Busy => BUSY.to_string(),
            BeginError::Lock(why) => format!("Cannot take Claude Code's storage lock: {why}"),
            BeginError::Store(error) => store_error(error),
            BeginError::Unavailable(why) => why,
        };
        let (document, pending) =
            claude_store::begin(&self.storage_dir(), &keychain, &held).map_err(begin_error)?;
        if document.is_none() && matches!(change, Change::SignedOut) {
            return Ok(()); // Nothing is stored, so nothing is signed in to empty.
        }
        let write = if self.in_home {
            pending.prove_in_home()
        } else {
            pending.prove()
        }
        .map_err(begin_error)?;
        let mut auth = document.unwrap_or_else(|| json!({}));
        let object = auth
            .as_object_mut()
            .ok_or("Malformed Claude credentials.")?;
        match change {
            Change::Login(Some(login)) => {
                object.insert(
                    "claudeAiOauth".into(),
                    login
                        .auth
                        .get("claudeAiOauth")
                        .cloned()
                        .ok_or("Missing Claude login.")?,
                );
            }
            Change::Login(None) => {
                object.remove("claudeAiOauth");
            }
            Change::SignedOut => {
                object.insert(
                    "claudeAiOauth".into(),
                    json!({"accessToken": "", "refreshToken": "", "expiresAt": 0}),
                );
            }
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
        if let Change::Login(login) = change {
            crate::config_io::ConfigIo::patch_account_identity(
                &self.config_file,
                login.map(|l| &l.account),
            )?;
            ensure()?;
        }
        write.commit(&auth)
    }
}

impl Native for ClaudeNative {
    fn lock(&self) -> Result<Box<dyn NativeGuard>, String> {
        ClaudeLocks::acquire(
            &self.storage_dir(),
            LockScope::RefreshAndConfig(&self.config_file),
        )
        .map(|guard| Box::new(guard) as Box<dyn NativeGuard>)
        .map_err(|error| match error {
            LockError::Busy => BUSY.into(),
            LockError::Unavailable(why) => format!("Cannot take Claude Code's locks: {why}"),
        })
    }

    /// Claude Code signs out by emptying `claudeAiOauth`, so a login needs an access token, by the
    /// same rule the Limits read applies. Only `claudeAiOauth` is read, never the MCP tokens
    /// beside it.
    fn read(&self) -> Result<Option<Login>, String> {
        let Some(auth) = self.stored()?.document else {
            return Ok(None);
        };
        let account = native::read_json(&self.config_file)?
            .get("oauthAccount")
            .cloned()
            .unwrap_or(Value::Null);
        // Claude Code signs out by emptying `claudeAiOauth` of its access token.
        if model::string(&auth, "/claudeAiOauth/accessToken").is_err() {
            return Ok(None);
        }
        Ok(Some(Login {
            auth: json!({"claudeAiOauth":auth["claudeAiOauth"]}),
            account,
        }))
    }

    fn identify(&self, login: &Login) -> Result<Identity, String> {
        ClaudeLogin::of(login).identity()
    }

    fn write(&self, login: Option<&Login>) -> Result<(), String> {
        self.write_locked(login, self.lock()?).map(drop)
    }

    fn write_locked(
        &self,
        login: Option<&Login>,
        locks: Box<dyn NativeGuard>,
    ) -> Result<ReadBack, String> {
        self.publish(Change::Login(login), locks.as_ref())?;
        let back = self.read();
        drop(locks);
        Ok(back)
    }

    /// Claude Code, asked what it has stored, must say it is signed in to the organization the
    /// login's account names, as its email, and the login must be the same once it has answered.
    /// Nothing is sent to Anthropic: a login it refuses shows at the next usage read.
    fn verify(&self) -> Result<(), String> {
        let login = self.read()?.ok_or("No native login was found.")?;
        let identity = self.identify(&login)?;
        let status = crate::limits::claude_cli::auth_status(&|| self.command())
            .ok_or("Claude Code could not say which account it is signed in to.")?;
        if status.get("loggedIn").and_then(Value::as_bool) != Some(true) {
            return Err("Claude Code does not report the login as signed in.".into());
        }
        if status.get("orgId").and_then(Value::as_str) != Some(identity.workspace_id.as_str()) {
            return Err("Claude credential and organization identity disagree.".into());
        }
        let email = ClaudeLogin::of(&login).email();
        if email.is_some() && status.get("email").and_then(Value::as_str) != email.as_deref() {
            return Err("Claude credential and account identity disagree.".into());
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
}

impl NativeAccount for ClaudeNative {
    fn preflight(&self) -> Result<(), String> {
        self.preflight_in(&crate::paths::process_env, Path::new(MANAGED_SETTINGS))
    }

    fn logout(&self) -> Result<(), String> {
        native::run(&mut self.logout_command(), Duration::from_secs(45)).map(|_| ())
    }
}

impl IsolatedSignIn for ClaudeNative {
    fn sign_in(&self) -> Command {
        let mut command = self.command();
        command.args(["auth", "login", "--claudeai"]);
        command
    }

    /// Claude Code's own usage report, run in this sign-in's config dir, which renews its login
    /// itself if it has to; no request is made with the login here.
    fn first_usage(&self, _dir: &Path, identity: &Identity) -> Option<ProviderLimitsDto> {
        crate::limits::claude_cli::read_usage(&|| self.command(), &self.config_file, identity).ok()
    }

    /// Deletes the sign-in's own scoped Keychain entry, never Claude Code's unscoped one.
    fn clean(&self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let service = self.service();
            if let Some(account) = claude_store::keychain_account(&service)? {
                super::keychain::delete(&service, &account)?;
            }
        }
        Ok(())
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

/// A Claude login read by Claude Code's rules.
pub(crate) struct ClaudeLogin<'a> {
    auth: &'a Value,
    account: &'a Value,
}

impl<'a> ClaudeLogin<'a> {
    pub(crate) fn of(login: &'a Login) -> Self {
        Self {
            auth: &login.auth,
            account: &login.account,
        }
    }
}

impl LoginView for ClaudeLogin<'_> {
    fn identity(&self) -> Result<Identity, String> {
        model::string(self.auth, "/claudeAiOauth/accessToken")?;
        model::string(self.auth, "/claudeAiOauth/refreshToken")?;
        Ok(Identity {
            provider: AgentId::Claude,
            user_id: model::string(self.account, "/accountUuid")?.to_owned(),
            workspace_id: model::string(self.account, "/organizationUuid")?.to_owned(),
        })
    }

    fn email(&self) -> Option<String> {
        self.account
            .get("emailAddress")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|email| !email.is_empty())
            .map(str::to_owned)
    }

    fn fingerprint(&self) -> String {
        model::claude_fingerprint(
            self.auth.pointer("/claudeAiOauth/accessToken"),
            self.auth.pointer("/claudeAiOauth/refreshToken"),
        )
    }

    /// Once its access token's `expiresAt` is reached; never for a login that states none.
    fn renewal_due(&self, now_ms: i64) -> bool {
        self.auth
            .pointer("/claudeAiOauth/expiresAt")
            .and_then(Value::as_i64)
            .is_some_and(|expires_at| expires_at <= now_ms)
    }
}

#[cfg(test)]
mod tests;
