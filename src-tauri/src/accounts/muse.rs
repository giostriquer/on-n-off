use super::{
    clients::Client,
    model::{self, AccessToken, Identity, LoginView},
    native::{self, CUSTOM_HOME},
    store::Login,
    transaction::{Native, NativeGuard},
    Home, IsolatedSignIn, NativeAccount,
};
use crate::dto::{AgentId, ProviderLimitsDto};
use crate::http::HttpError;
use crate::limits::{SavedReadError, SavedReadUrls};
use serde_json::{json, Map, Value};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const ENV_CREDENTIALS: [&str; 1] = ["META_API_KEY"];
const OVERRIDES: [&str; 3] = ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "MUSE_AUTH_PATH"];
const SECRETS: [&str; 2] = ["access_token", "api_key"];
#[cfg(target_os = "macos")]
use super::muse_store::{KEYCHAIN_ACCOUNT, KEYCHAIN_SERVICE};

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
    ) -> Result<ProviderLimitsDto, SavedReadError> {
        let token = MuseView::of(login)
            .access_token()
            .ok_or(HttpError::Unauthorized)?;
        crate::limits::read_saved_muse(identity, &token, urls)
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
    key_url: &'static str,
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
            key_url: SavedReadUrls::LIVE.muse,
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
            key_url: SavedReadUrls::LIVE.muse,
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

    fn preflight_in(&self, env: &dyn Fn(&str) -> Option<OsString>) -> Result<(), String> {
        if self.custom {
            return Err(CUSTOM_HOME.into());
        }
        native::refuse_linked(&self.auth_file())?;
        native::refuse_env_credentials(&ENV_CREDENTIALS, env)
    }

    fn document(&self) -> Result<Value, String> {
        let document = native::read_json(&self.auth_file())?;
        if document.is_object() {
            Ok(document)
        } else {
            Err("The Muse login document is malformed. It has not been changed.".into())
        }
    }

    fn keychain_bundle(&self) -> Result<Option<Value>, String> {
        #[cfg(target_os = "macos")]
        if self.keychain {
            return super::keychain::find_password(KEYCHAIN_SERVICE, Some(KEYCHAIN_ACCOUNT))?
                .map(|raw| {
                    serde_json::from_str(raw.trim()).map_err(|_| {
                        "The Muse Keychain login is malformed. It has not been changed.".to_string()
                    })
                })
                .transpose();
        }
        Ok(None)
    }

    fn put_keychain(&self, bundle: Option<&Value>) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        if self.keychain {
            return match bundle {
                Some(bundle) => super::keychain::write(
                    KEYCHAIN_SERVICE,
                    KEYCHAIN_ACCOUNT,
                    &serde_json::to_vec(bundle).map_err(|_| "Cannot encode the Muse login.")?,
                ),
                None => super::keychain::delete(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT),
            };
        }
        let _ = bundle;
        Ok(())
    }

    fn write_document(&self, document: &Value) -> Result<(), String> {
        let file = self.auth_file();
        if fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Refusing to replace a linked Muse login file.".into());
        }
        let bytes =
            serde_json::to_vec_pretty(document).map_err(|_| "Cannot encode the Muse login.")?;
        super::vault::atomic_write(&file, &bytes)
    }
}

impl Native for MuseNative {
    fn read(&self) -> Result<Option<Login>, String> {
        let document = self.document()?;
        let Some(meta) = document
            .pointer("/providers/meta")
            .and_then(Value::as_object)
        else {
            return Ok(None);
        };
        let inline = model::string(&Value::Object(meta.clone()), "/access_token").is_ok();
        let auth = if inline {
            secret(meta.get("access_token"), meta.get("api_key"), None)
        } else if meta.get("storage").and_then(Value::as_str) == Some("keychain") {
            match self.keychain_bundle()? {
                Some(bundle) => secret(
                    bundle.get("access_token"),
                    bundle.get("api_key"),
                    bundle.get("secret_schema_version"),
                ),
                None => return Ok(None),
            }
        } else {
            Value::Object(Map::new())
        };
        Ok(Some(Login {
            auth,
            account: Value::Object(metadata(meta)),
        }))
    }

    fn identify(&self, login: &Login) -> Result<Identity, String> {
        MuseView::of(login).identity()
    }

    fn write(&self, login: Option<&Login>) -> Result<(), String> {
        let mut document = self.document()?;
        let providers = document
            .as_object_mut()
            .ok_or("The Muse login document is malformed.")?
            .entry("providers")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or("The Muse login document is malformed.")?;
        let Some(login) = login else {
            providers.remove("meta");
            self.put_keychain(None)?;
            return self.write_document(&document);
        };
        let mut meta = login.account.as_object().cloned().unwrap_or_default();
        for name in SECRETS.iter().chain(&["storage"]) {
            meta.remove(*name);
        }
        if self.keychain {
            let version = self
                .keychain_bundle()?
                .and_then(|bundle| bundle.get("secret_schema_version").cloned())
                .or_else(|| login.auth.get("secret_schema_version").cloned())
                .unwrap_or_else(|| json!(1));
            self.put_keychain(Some(&secret(
                login.auth.get("access_token"),
                login.auth.get("api_key"),
                Some(&version),
            )))?;
            meta.insert("storage".into(), json!("keychain"));
        } else {
            for name in SECRETS {
                if let Some(value) = login.auth.get(name) {
                    meta.insert(name.into(), value.clone());
                }
            }
        }
        providers.insert("meta".into(), Value::Object(meta));
        document
            .as_object_mut()
            .ok_or("The Muse login document is malformed.")?
            .entry("schema_version")
            .or_insert_with(|| json!(1));
        self.write_document(&document)
    }

    fn verify(&self) -> Result<(), String> {
        let login = self.read()?.ok_or("Muse has no login to verify.")?;
        let identity = self.identify(&login)?;
        let token = MuseView::of(&login)
            .access_token()
            .ok_or("The Muse login has no access token.")?;
        let urls = SavedReadUrls {
            muse: self.key_url,
            ..SavedReadUrls::LIVE
        };
        match crate::limits::read_saved_muse(&identity, &token, &urls) {
            Ok(_) | Err(SavedReadError::Unavailable(_)) => Ok(()),
            Err(SavedReadError::OtherAccount) => {
                Err("Muse answered for a different account. Sign in again.".into())
            }
            Err(_) => Err(
                "The Muse login could not be verified. Check connectivity or sign in again.".into(),
            ),
        }
    }
}

impl NativeAccount for MuseNative {
    fn preflight(&self) -> Result<(), String> {
        self.preflight_in(&crate::paths::process_env)
    }

    fn logout(&self) -> Result<(), String> {
        let mut command = self.command();
        command.arg("logout");
        native::run(&mut command, Duration::from_secs(45)).map(|_| ())
    }

    fn subscription(&self) -> Result<Option<Identity>, String> {
        match self.read()? {
            Some(login) if MuseView::of(&login).oauth() => self.identify(&login).map(Some),
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

    fn sign_in_input(&self) -> Option<&'static [u8]> {
        Some(b"\n")
    }

    fn first_usage(&self, _dir: &Path, identity: &Identity) -> Option<ProviderLimitsDto> {
        let login = self.read().ok()??;
        let token = MuseView::of(&login).access_token()?;
        crate::limits::read_saved_muse(identity, &token, &SavedReadUrls::LIVE).ok()
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

    fn read_usage(&self, identity: &Identity) -> Result<ProviderLimitsDto, SavedReadError> {
        let login = self
            .store
            .read()
            .map_err(|_| SavedReadError::Unavailable("Cannot read the saved Muse login."))?
            .ok_or(HttpError::Unauthorized)?;
        let token = MuseView::of(&login)
            .access_token()
            .ok_or(HttpError::Unauthorized)?;
        let urls = SavedReadUrls {
            muse: self.store.key_url,
            ..SavedReadUrls::LIVE
        };
        crate::limits::read_saved_muse(identity, &token, &urls)
    }

    fn sign_in_deadline(&self) -> Option<model::SignInDeadline> {
        None
    }

    fn delete(&self, locks: Box<dyn NativeGuard>) -> Result<(), String> {
        let removed = match fs::remove_dir_all(&self.dir) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err("Cannot remove the saved account's home.".into())
            }
            _ => Ok(()),
        };
        drop(locks);
        removed
    }
}

fn secret(access: Option<&Value>, api_key: Option<&Value>, version: Option<&Value>) -> Value {
    let mut secret = Map::new();
    for (name, value) in [
        ("access_token", access),
        ("api_key", api_key),
        ("secret_schema_version", version),
    ] {
        if let Some(value) = value.filter(|value| !value.is_null()) {
            secret.insert(name.into(), value.clone());
        }
    }
    Value::Object(secret)
}

fn metadata(meta: &Map<String, Value>) -> Map<String, Value> {
    let mut metadata = meta.clone();
    for name in SECRETS.iter().chain(&["storage"]) {
        metadata.remove(*name);
    }
    metadata
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

    pub(crate) fn access_token(&self) -> Option<AccessToken> {
        model::string(self.auth, "/access_token")
            .ok()
            .map(AccessToken::new)
    }

    fn oauth(&self) -> bool {
        self.account.get("mechanism").and_then(Value::as_str) == Some("oauth")
    }
}

impl LoginView for MuseView<'_> {
    fn identity(&self) -> Result<Identity, String> {
        if !self.oauth() {
            return Err("API key logins cannot be saved as subscription profiles.".into());
        }
        model::string(self.auth, "/access_token")?;
        let email = self
            .email()
            .filter(|email| email.contains('@'))
            .ok_or("This Muse login has no account email, so it cannot be saved.")?;
        Ok(crate::limits::muse_identity(&email))
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
