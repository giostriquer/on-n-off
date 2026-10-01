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

    fn renew_private(&self, _: &Login, _: i64, _: &str) -> Result<Login, String> {
        Err("Claude Code renews a saved Claude login itself, in the account's home.".into())
    }

    fn homes(&self) -> Option<super::HomeAt> {
        Some(|dir| Box::new(home::ClaudeHome::at(dir)))
    }

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

    fn client(&self) -> &'static Client {
        &Client {
            name: "claude",
            package_entry: "/@anthropic-ai/claude-code/cli.js",
            blocks_activation: false,
        }
    }
}

const BUSY: &str = "Claude is updating its login or configuration. Retry after it finishes.";

const LOST: &str = "Native credential coordination was lost. Protected recovery has been retained.";

const MANAGED_SETTINGS: &str = "/Library/Application Support/ClaudeCode/managed-settings.json";

fn store_error(error: StoreError) -> String {
    match error {
        StoreError::Keychain(why) => why,
        StoreError::FileUnreadable(_) => "Cannot read native credentials.".into(),
        StoreError::FileMalformed(_) => {
            "The native credential document is malformed. It has not been changed.".into()
        }
    }
}

pub(super) struct ClaudeNative {
    config_home: PathBuf,
    config_file: PathBuf,
    custom: bool,
    use_keychain: bool,
    secure_storage: Option<SecureStorage>,
    in_home: bool,
    private: bool,
}

pub(crate) struct SignedIn(ClaudeNative);

impl SignedIn {
    pub(crate) fn resolve(home: &Path) -> Result<Self, String> {
        ClaudeNative::resolve(home).map(Self)
    }

    pub(crate) fn command(&self) -> Command {
        let mut command = self.0.command();
        if !self.0.config_home.is_dir() {
            command.current_dir(std::env::temp_dir());
        }
        command
    }

    pub(crate) fn usage_command(&self, config_home: &Path) -> Command {
        let mut command = self.0.command();
        command
            .env("CLAUDE_CONFIG_DIR", config_home)
            .env(
                claude_store::SECURE_STORAGE_VAR,
                self.0.storage_dir().secure_storage_var(),
            )
            .current_dir(config_home);
        command
    }

    pub(crate) fn config_file(&self) -> &Path {
        &self.0.config_file
    }
}

#[derive(Clone, Copy)]
enum Change<'a> {
    Login(Option<&'a Login>),
    SignedOut,
}

impl ClaudeNative {
    pub(super) fn resolve(home: &Path) -> Result<Self, String> {
        Self::resolve_from(home, &crate::paths::process_env)
    }

    fn resolve_from(
        home: &Path,
        lookup: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<Self, String> {
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

    fn isolated(dir: &Path) -> Result<Self, String> {
        let config_home = dir.join(".claude");
        fs::create_dir_all(&config_home).map_err(|_| "Cannot create isolated login home.")?;
        Ok(Self {
            in_home: false,
            ..Self::private(config_home)
        })
    }

    fn home(dir: &Path) -> Self {
        Self {
            in_home: true,
            ..Self::private(dir.join(".claude"))
        }
    }

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

    fn preflight_in(
        &self,
        env: &dyn Fn(&str) -> Option<OsString>,
        managed_settings: &Path,
    ) -> Result<(), String> {
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

    fn storage_dir(&self) -> StorageDir {
        StorageDir::of(&self.config_home, self.custom, self.secure_storage.as_ref())
    }

    fn storage_moved(&self) -> bool {
        self.storage_dir() != StorageDir::new(self.config_home.clone(), self.custom)
    }

    fn stored(&self) -> Result<Stored, String> {
        claude_store::read(&self.storage_dir(), self.keychain()).map_err(store_error)
    }

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

    fn command(&self) -> Command {
        let mut command = native::cli(AgentId::Claude, "claude");
        if self.custom || !self.use_keychain {
            command.env("CLAUDE_CONFIG_DIR", &self.config_home);
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
        match &self.secure_storage {
            Some(secure) => command.env(claude_store::SECURE_STORAGE_VAR, &secure.var),
            None => command.env_remove(claude_store::SECURE_STORAGE_VAR),
        };
        command.current_dir(&self.config_home);
        command
    }

    fn logout_command(&self) -> Command {
        let mut command = self.command();
        command.args(["auth", "logout"]);
        command
    }

    fn publish(&self, change: Change<'_>, locks: &dyn NativeGuard) -> Result<(), String> {
        locks.ensure()?;
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
            return Ok(());
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

    fn read(&self) -> Result<Option<Login>, String> {
        let Some(auth) = self.stored()?.document else {
            return Ok(None);
        };
        let account = native::read_json(&self.config_file)?
            .get("oauthAccount")
            .cloned()
            .unwrap_or(Value::Null);
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

    fn first_usage(&self, _dir: &Path, identity: &Identity) -> Option<ProviderLimitsDto> {
        crate::limits::claude_cli::read_usage(&|| self.command(), &self.config_file, identity).ok()
    }

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

    fn renewal_due(&self, now_ms: i64) -> bool {
        self.auth
            .pointer("/claudeAiOauth/expiresAt")
            .and_then(Value::as_i64)
            .is_some_and(|expires_at| expires_at <= now_ms)
    }
}

#[cfg(test)]
mod tests;
