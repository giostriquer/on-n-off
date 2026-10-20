use super::{
    clients::Client,
    model::{self, AccessToken, Identity, LoginView},
    muse_store::{self, Keychain, MuseCredential, SystemKeychain, SIGNED_IN},
    native::{self, CUSTOM_HOME},
    store::Login,
    transaction::{Native, NativeGuard},
    Home, IsolatedSignIn, NativeAccount,
};
use crate::dto::AgentId;
use crate::limits::{read_saved_muse, verify_saved_muse, ReadFailure, SavedReadUrls};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const ENV_CREDENTIALS: [&str; 1] = ["META_API_KEY"];
const OVERRIDES: [&str; 3] = ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "MUSE_AUTH_PATH"];
const MALFORMED_LOGIN: &str = "The saved Muse login is malformed.";
const SIGN_IN_PAGE_PREFIX: &str = "https://auth.meta.com/";

pub(super) struct Muse;

impl super::Adapter for Muse {
    fn native(&self, home: &Path) -> Result<Box<dyn NativeAccount>, String> {
        Ok(Box::new(MuseNative::resolve(home)?))
    }

    fn isolated(&self, dir: &Path) -> Result<Box<dyn IsolatedSignIn>, String> {
        Ok(Box::new(MuseNative::isolated(dir)?))
    }

    fn login<'a>(&self, login: &'a Login) -> Box<dyn LoginView + 'a> {
        Box::new(MuseView::of(login))
    }

    fn client(&self) -> &'static Client {
        &Client {
            name: "muse",
            package_entry: "/muse/bin/muse",
            binary_prefix: Some("muse-bin-"),
            blocks_activation: true,
        }
    }

    fn token_url(&self) -> Option<&'static str> {
        None
    }

    fn renew_private(&self, _: &Login, _: i64, _: &str) -> Result<Login, String> {
        Err("on-n-off does not renew Muse logins. Sign in again.".into())
    }

    fn read_usage(
        &self,
        identity: &Identity,
        login: &Login,
        _now_ms: i64,
        urls: &SavedReadUrls<'_>,
    ) -> Result<crate::limits::Accepted, ReadFailure> {
        let token = MuseView::of(login)
            .access_token()
            .ok_or(ReadFailure::LoginRefused)?;
        read_saved_muse(identity, &token, urls)
    }

    fn homes(&self) -> Option<super::HomeAt> {
        Some(|dir| Box::new(MuseHome::at(dir)))
    }
}

pub(super) struct MuseNative {
    config: PathBuf,
    private_home: Option<PathBuf>,
    custom: bool,
    keychain: bool,
    urls: SavedReadUrls<'static>,
}

impl MuseNative {
    pub(super) fn resolve(home: &Path) -> Result<Self, String> {
        Self::resolve_from(home, &crate::paths::process_env)
    }

    fn resolve_from(
        home: &Path,
        lookup: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<Self, String> {
        let disposable = lookup("ON_N_OFF_HOME").is_some();
        let config = crate::paths::muse_config_for(home);
        if !config.is_absolute() {
            return Err("The provider home must be an absolute path.".into());
        }
        Ok(Self {
            config,
            private_home: None,
            custom: !disposable && OVERRIDES.iter().any(|name| lookup(name).is_some()),
            keychain: cfg!(target_os = "macos") && !disposable,
            urls: SavedReadUrls::LIVE,
        })
    }

    fn isolated(dir: &Path) -> Result<Self, String> {
        let store = Self::private(dir);
        fs::create_dir_all(&store.config).map_err(|_| "Cannot create isolated login home.")?;
        fs::create_dir_all(dir.join("data")).map_err(|_| "Cannot create isolated login home.")?;
        Ok(store)
    }

    fn private(dir: &Path) -> Self {
        Self {
            config: dir.join("config").join("muse"),
            private_home: Some(dir.to_path_buf()),
            custom: true,
            keychain: false,
            urls: SavedReadUrls::LIVE,
        }
    }

    fn auth_file(&self) -> PathBuf {
        self.config.join("auth.json")
    }

    fn command(&self) -> Command {
        let mut command = native::cli(AgentId::Muse, "muse");
        if let Some(home) = &self.private_home {
            command
                .env("HOME", home)
                .env("XDG_CONFIG_HOME", home.join("config"))
                .env("XDG_DATA_HOME", home.join("data"))
                .env("TBH_CREDENTIAL_BACKEND", "file")
                .env_remove("MUSE_AUTH_PATH")
                .current_dir(home);
            #[cfg(windows)]
            command.env("USERPROFILE", home);
        }
        command
    }

    fn logout_command(&self) -> Command {
        let mut command = self.command();
        command.arg("logout");
        command
    }

    fn preflight_in(&self, env: &dyn Fn(&str) -> Option<OsString>) -> Result<(), String> {
        if self.custom {
            return Err(CUSTOM_HOME.into());
        }
        native::refuse_linked(&self.auth_file())?;
        native::refuse_env_credentials(&ENV_CREDENTIALS, env)
    }

    fn keychain(&self) -> Option<&'static dyn Keychain> {
        self.keychain.then_some(&SystemKeychain as &dyn Keychain)
    }

    fn token(&self) -> Option<AccessToken> {
        let login = self.read().ok()??;
        MuseView::of(&login).access_token()
    }
}

impl Native for MuseNative {
    fn read(&self) -> Result<Option<Login>, String> {
        Ok(SIGNED_IN
            .native(&self.config, self.keychain())?
            .map(|native| Login {
                auth: Value::Object(native.secret),
                account: Value::Object(native.metadata),
            }))
    }

    fn identify(&self, login: &Login) -> Result<Identity, String> {
        MuseView::of(login).identity()
    }

    fn write(&self, login: Option<&Login>) -> Result<(), String> {
        let parts = match login {
            Some(login) => Some((
                login.account.as_object().ok_or(MALFORMED_LOGIN)?,
                login.auth.as_object().ok_or(MALFORMED_LOGIN)?,
            )),
            None => None,
        };
        SIGNED_IN.write(&self.config, self.keychain(), parts)
    }

    fn verify(&self) -> Result<(), String> {
        let login = self.read()?.ok_or("Muse has no login to verify.")?;
        let identity = self.identify(&login)?;
        let token = MuseView::of(&login)
            .access_token()
            .ok_or("The Muse login has no access token.")?;
        verify_saved_muse(&identity, &token, &self.urls)
    }
}

impl NativeAccount for MuseNative {
    fn preflight(&self) -> Result<(), String> {
        self.preflight_in(&crate::paths::process_env)
    }

    fn logout(&self) -> Result<(), String> {
        native::run(&mut self.logout_command(), Duration::from_secs(45)).map(|_| ())
    }

    fn subscription(&self) -> Result<Option<Identity>, String> {
        match self.read()? {
            Some(login) if MuseView::of(&login).access_token().is_some() => {
                self.identify(&login).map(Some)
            }
            _ => Ok(None),
        }
    }
}

impl IsolatedSignIn for MuseNative {
    fn sign_in(&self) -> Command {
        let mut command = self.command();
        command.arg("login");
        command
    }

    fn sign_in_page<'a>(&self, line: &'a str) -> Option<&'a str> {
        let page = line.trim();
        (page.starts_with(SIGN_IN_PAGE_PREFIX) && !page.contains(char::is_whitespace))
            .then_some(page)
    }

    fn first_usage(&self, _dir: &Path, identity: &Identity) -> Option<crate::limits::Accepted> {
        read_saved_muse(identity, &self.token()?, &self.urls).ok()
    }

    fn clean(&self) -> Result<(), String> {
        Ok(())
    }
}

pub(in crate::accounts) struct MuseHome {
    dir: PathBuf,
    store: MuseNative,
}

impl MuseHome {
    pub(super) fn at(dir: &Path) -> Self {
        Self {
            dir: dir.into(),
            store: MuseNative::private(dir),
        }
    }
}

impl Home for MuseHome {
    fn lock(&self) -> Result<Box<dyn NativeGuard>, String> {
        Ok(Box::new(()))
    }

    fn read(&self) -> Result<Option<Login>, String> {
        self.store.read()
    }

    fn identify(&self, login: &Login) -> Result<Identity, String> {
        self.store.identify(login)
    }

    fn put(&self, login: &Login, locks: &dyn NativeGuard) -> Result<Option<Login>, String> {
        locks.ensure()?;
        self.store.write(Some(login))?;
        self.store.read()
    }

    fn clear(&self, locks: &dyn NativeGuard) -> Result<(), String> {
        locks.ensure()?;
        self.store.write(None)
    }

    fn read_usage(&self, identity: &Identity) -> Result<crate::limits::Accepted, ReadFailure> {
        let token = self.store.token().ok_or(ReadFailure::LoginRefused)?;
        read_saved_muse(identity, &token, &self.store.urls)
    }

    fn sign_in_deadline(&self) -> Option<model::SignInDeadline> {
        None
    }

    fn delete(&self, locks: Box<dyn NativeGuard>) -> Result<(), String> {
        super::homes::remove(&self.dir, locks)
    }
}

pub(crate) struct MuseView<'a> {
    auth: &'a Value,
    account: &'a Value,
}

impl<'a> MuseView<'a> {
    pub(crate) fn of(login: &'a Login) -> Self {
        Self {
            auth: &login.auth,
            account: &login.account,
        }
    }

    fn credential(&self) -> Option<MuseCredential> {
        self.auth.as_object().and_then(muse_store::credential_of)
    }

    pub(crate) fn access_token(&self) -> Option<AccessToken> {
        match self.credential()? {
            MuseCredential::Token(token) => Some(token),
            _ => None,
        }
    }
}

impl LoginView for MuseView<'_> {
    fn identity(&self) -> Result<Identity, String> {
        match self.credential() {
            Some(MuseCredential::Token(_)) => self
                .email()
                .as_deref()
                .and_then(muse_store::identity)
                .ok_or_else(|| {
                    "This Muse login has no account email, so it cannot be saved.".into()
                }),
            Some(MuseCredential::ApiKey) => {
                Err("API key logins cannot be saved as subscription profiles.".into())
            }
            _ => Err(model::MISSING_CREDENTIALS.into()),
        }
    }

    fn email(&self) -> Option<String> {
        self.account
            .get("user_email")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|email| !email.is_empty())
            .map(str::to_owned)
    }

    fn fingerprint(&self) -> String {
        model::muse_fingerprint(self.auth.get("access_token"), self.auth.get("api_key"))
    }

    fn renewal_due(&self, _now_ms: i64) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
