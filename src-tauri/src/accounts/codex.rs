//! The Codex adapter: Codex's half of the accounts seam. It reads a Codex login by Codex's rules
//! ([`CodexLogin`]): `auth.json` as Codex writes it, whose ID token's claims say who it is. And it
//! is the account switch's view of Codex's native store ([`CodexNative`]): which home the
//! environment selects and when account changes defer to the official client, the `codex` it
//! starts, the read, the write and its verification, and an isolated sign-in. Which backend holds
//! the login is `codex_store`'s question; this adapter asks it.
use super::{
    clients::Client,
    codex_store,
    model::{self, AccessToken, Identity, LoginView},
    native::{self, CUSTOM_HOME},
    store::{Login, Store},
    transaction::{Native, NativeGuard, ReadBack},
    usage_renew, IsolatedSignIn, NativeAccount,
};
use crate::dto::{AgentId, LimitsStatus, ProviderLimitsDto};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub(super) struct Codex;

impl super::Adapter for Codex {
    fn native(&self, home: &Path) -> Result<Box<dyn NativeAccount>, String> {
        Ok(Box::new(CodexNative::resolve(home)?))
    }

    fn login<'a>(&self, login: &'a Login) -> Box<dyn LoginView + 'a> {
        Box::new(CodexLogin::of(login))
    }

    fn token_url(&self) -> &'static str {
        "https://auth.openai.com/oauth/token"
    }

    /// Codex's own JSON refresh grant, built and folded by the login and sent by `usage_renew`.
    fn renew_private(&self, login: &Login, now_ms: i64, token_url: &str) -> Result<Login, String> {
        let login = CodexLogin::of(login);
        let reply = usage_renew::grant(token_url, &login.renewal_request()?)
            .map_err(|_| "Could not renew the private Codex login. Sign in again if needed.")?;
        login.renewed(&reply, now_ms)
    }

    /// Running Codex clients never pick up a replaced login, so they refuse an ordinary switch.
    fn client(&self) -> &'static Client {
        &Client {
            name: "codex",
            package_entry: "/@openai/codex/bin/codex.js",
            blocks_activation: true,
        }
    }
}

/// The credentials that override Codex's own login when set in its environment.
const ENV_CREDENTIALS: [&str; 3] = ["OPENAI_API_KEY", "CODEX_API_KEY", "CODEX_AUTH_TOKEN"];

/// Where an administrator's managed Codex configuration lives.
const MANAGED_CONFIG: [&str; 2] = [
    "/etc/codex/requirements.toml",
    "/etc/codex/managed_config.toml",
];

/// Codex's native store as the account switch uses it: the user's own, where the environment
/// puts it ([`CodexNative::resolve`]), or an isolated sign-in's.
pub(super) struct CodexNative {
    config_home: PathBuf,
    config_file: PathBuf,
    /// `CODEX_HOME` chose the home, or this is an isolated sign-in's.
    custom: bool,
}

impl CodexNative {
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

    /// A private store in `dir` for one isolated sign-in.
    fn isolated(dir: &Path) -> Result<Self, String> {
        let config_home = dir.join(".codex");
        fs::create_dir_all(&config_home).map_err(|_| "Cannot create isolated login home.")?;
        Ok(Self {
            config_file: codex_store::config_file(&config_home),
            config_home,
            custom: true,
        })
    }

    /// `preflight` against the environment `env` reads and the managed configuration files at
    /// `managed`.
    fn preflight_in(
        &self,
        env: &dyn Fn(&str) -> Option<OsString>,
        managed: &[&Path],
    ) -> Result<(), String> {
        if self.custom {
            return Err(CUSTOM_HOME.into());
        }
        native::refuse_linked(&self.config_file)?;
        native::refuse_env_credentials(&ENV_CREDENTIALS, env)?;
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
        if managed.iter().any(|path| path.exists()) {
            return Err(
                "Managed Codex authentication must be changed through the official client.".into(),
            );
        }
        Ok(())
    }

    /// A `codex` working in this store's home.
    fn command(&self) -> Command {
        let mut command = native::cli(AgentId::Codex, "codex");
        command.env("CODEX_HOME", &self.config_home);
        command.current_dir(&self.config_home);
        command
    }

    /// `codex logout`.
    fn logout_command(&self) -> Command {
        let mut command = self.command();
        command.arg("logout");
        command
    }

    /// `codex login status`, which succeeds only while a login is signed in.
    fn status_command(&self) -> Command {
        let mut command = self.command();
        command.args(["login", "status"]);
        command
    }

    /// Whether Codex's own signed-in usage read succeeds: its app-server renews and reads the
    /// login, so on-n-off never renews a native Codex login itself. `force` skips the shared cache.
    fn verify_signed_in(&self, force: bool) -> Result<(), String> {
        let entries = crate::limits::read_limits(AgentId::Codex, force);
        if entries
            .iter()
            .any(|v| v.current_account && v.status == LimitsStatus::Ok)
        {
            Ok(())
        } else {
            Err("The CLI login could not be verified. Check connectivity or sign in again.".into())
        }
    }
}

impl Native for CodexNative {
    /// Whatever document the store its config selects holds.
    fn read(&self) -> Result<Option<Login>, String> {
        codex_store::read(&self.config_home)
    }

    fn identify(&self, login: &Login) -> Result<Identity, String> {
        CodexLogin::of(login).identity()
    }

    fn write(&self, login: Option<&Login>) -> Result<(), String> {
        self.write_locked(login, self.lock()?).map(drop)
    }

    /// The login's document, verbatim, to the store its config selects.
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
            return native::run(&mut self.status_command(), Duration::from_secs(30)).map(|_| ());
        }
        self.verify_signed_in(true)
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
            self.verify_signed_in(false)
        }
    }
}

impl NativeAccount for CodexNative {
    fn preflight(&self) -> Result<(), String> {
        let managed = MANAGED_CONFIG.map(Path::new);
        self.preflight_in(&crate::paths::process_env, &managed)
    }

    fn logout(&self) -> Result<(), String> {
        native::run(&mut self.logout_command(), Duration::from_secs(45)).map(|_| ())
    }

    fn isolated(&self, dir: &Path) -> Result<Box<dyn IsolatedSignIn>, String> {
        Ok(Box::new(Self::isolated(dir)?))
    }

    /// An API-key login has no subscription, so it is no one's: every saved account is read.
    fn subscription(&self) -> Result<Option<Identity>, String> {
        match self.read()? {
            Some(login) if CodexLogin::of(&login).api_key() => Ok(None),
            Some(login) => self.identify(&login).map(Some),
            None => Ok(None),
        }
    }
}

impl IsolatedSignIn for CodexNative {
    fn sign_in(&self) -> Command {
        let mut command = self.command();
        command.arg("login");
        command
    }

    /// Read by Codex's own app-server in `dir`, which needs nothing from the login itself.
    fn first_usage(
        &self,
        dir: &Path,
        _login: &Login,
        identity: &Identity,
    ) -> Option<ProviderLimitsDto> {
        crate::limits::login::read(dir, identity, None)
    }

    /// Codex keeps an isolated login inside its home, so nothing is left outside it.
    fn clean(&self) -> Result<(), String> {
        Ok(())
    }
}

/// A Codex login read by Codex's rules.
pub(crate) struct CodexLogin<'a> {
    auth: &'a Value,
    account: &'a Value,
}

/// The account record of a login read from its credential alone. Codex keeps none.
static NO_ACCOUNT: Value = Value::Null;

impl<'a> CodexLogin<'a> {
    pub(crate) fn of(login: &'a Login) -> Self {
        Self {
            auth: &login.auth,
            account: &login.account,
        }
    }

    /// The credential alone, as a saved account's usage read holds it.
    pub(crate) fn of_auth(auth: &'a Value) -> Self {
        Self {
            auth,
            account: &NO_ACCOUNT,
        }
    }

    /// The access token, for one request header; `None` for a login without one.
    pub(crate) fn access_token(&self) -> Option<AccessToken> {
        model::string(self.auth, "/tokens/access_token")
            .ok()
            .map(AccessToken::new)
    }

    /// Whether this is an API-key login: a subscription login has no key, or a blank one.
    fn api_key(&self) -> bool {
        self.auth
            .get("OPENAI_API_KEY")
            .and_then(Value::as_str)
            .is_some_and(|key| !key.trim().is_empty())
    }

    /// The workspace the tokens are for, `tokens.account_id`, when it names one.
    pub(super) fn workspace(&self) -> Option<&'a str> {
        self.auth
            .pointer("/tokens/account_id")
            .and_then(Value::as_str)
            .filter(|workspace| !workspace.trim().is_empty())
    }

    /// Whether the login carries an ID token at all, readable or not.
    pub(super) fn has_id_token(&self) -> bool {
        self.auth.pointer("/tokens/id_token").is_some()
    }

    /// The ID token's `https://api.openai.com/auth` claims: who the login is, its workspace and its
    /// plan, and never a token. `None` when the token has no such claims; an error when there is
    /// no readable ID token.
    pub(crate) fn auth_claims(&self) -> Result<Option<Value>, String> {
        Ok(self.claims()?.get("https://api.openai.com/auth").cloned())
    }

    /// Whether a running Codex client may renew this login within ten minutes of `now` (Unix
    /// seconds). Codex renews shortly before expiry, five minutes in its source, and the desktop
    /// app sooner. An unreadable expiry counts as soon.
    pub(super) fn renews_soon(&self, now: i64) -> bool {
        token_claims(self.auth, "/tokens/access_token")
            .ok()
            .and_then(|claims| claims.get("exp")?.as_i64())
            .is_none_or(|expiry| expiry < now + 600)
    }

    /// The grant a private renewal sends: Codex's own JSON refresh grant, as its
    /// `login/src/oauth/client.rs` sends it.
    pub(super) fn renewal_request(&self) -> Result<Value, String> {
        let token = model::string(self.auth, "/tokens/refresh_token")?;
        Ok(serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": token,
            "client_id": "app_EMoamEEZ73f0CkXaXp7hrann",
        }))
    }

    /// This login with the grant's `reply` folded in: the access token, and the refresh and ID
    /// tokens when the reply carries them, replace the stored ones, `last_refresh` records `now_ms`,
    /// and every other field is left as it was.
    pub(super) fn renewed(&self, reply: &Value, now_ms: i64) -> Result<Login, String> {
        let mut auth = self.auth.clone();
        let access = model::string(reply, "/access_token")?.to_owned();
        auth["tokens"]["access_token"] = Value::String(access);
        for name in ["refresh_token", "id_token"] {
            if let Some(value) = reply
                .get(name)
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                auth["tokens"][name] = Value::String(value.to_owned());
            }
        }
        auth["last_refresh"] = Value::String(
            chrono::DateTime::from_timestamp_millis(now_ms)
                .ok_or("Invalid renewal time.")?
                .to_rfc3339(),
        );
        Ok(Login {
            auth,
            account: self.account.clone(),
        })
    }

    fn claims(&self) -> Result<Value, String> {
        token_claims(self.auth, "/tokens/id_token")
    }
}

impl LoginView for CodexLogin<'_> {
    fn identity(&self) -> Result<Identity, String> {
        model::string(self.auth, "/tokens/access_token")?;
        model::string(self.auth, "/tokens/refresh_token")?;
        if self
            .auth
            .get("OPENAI_API_KEY")
            .is_some_and(|v| !v.is_null())
        {
            return Err("API key logins cannot be saved as subscription profiles.".into());
        }
        let claims = self.claims()?;
        let user = model::string(&claims, "/https:~1~1api.openai.com~1auth/chatgpt_user_id")?;
        let workspace = model::string(
            &claims,
            "/https:~1~1api.openai.com~1auth/chatgpt_account_id",
        )?;
        if model::string(self.auth, "/tokens/account_id")? != workspace {
            return Err(
                "Native workspace and login claims disagree. Open the CLI to resolve the login."
                    .into(),
            );
        }
        Ok(Identity {
            provider: AgentId::Codex,
            user_id: user.to_owned(),
            workspace_id: workspace.to_owned(),
        })
    }

    /// The ID token's own `email` claim.
    fn email(&self) -> Option<String> {
        self.claims().ok().and_then(|claims| {
            claims
                .get("email")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|email| !email.is_empty())
                .map(str::to_owned)
        })
    }

    fn fingerprint(&self) -> String {
        model::codex_fingerprint(
            self.auth.pointer("/tokens/access_token"),
            self.auth.pointer("/tokens/refresh_token"),
        )
    }

    fn renewal_due(&self, now_ms: i64) -> bool {
        self.renews_soon(now_ms / 1000)
    }
}

/// The payload of the JWT at `pointer`.
fn token_claims(auth: &Value, pointer: &str) -> Result<Value, String> {
    let token = model::string(auth, pointer)?;
    let encoded = token
        .split('.')
        .nth(1)
        .ok_or("Invalid native identity token.")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded.trim_end_matches('='))
        .map_err(|_| "Invalid native identity token.")?;
    serde_json::from_slice(&bytes).map_err(|_| "Invalid native identity token.".into())
}

/// The `https://api.openai.com/auth` claims of a saved Codex profile's ID token: identity and plan
/// metadata, never its tokens. `None` for an unknown key, a profile without a login, or a device
/// with no vault; an error when the vault exists and cannot be read right now.
pub(crate) fn saved_claims(home: &Path, key: &str) -> Result<Option<Value>, String> {
    if !Identity::is_profile_key(key) || !Store::vault_exists(home) {
        return Ok(None);
    }
    let db = Store::open_existing(home)?.load()?;
    Ok(db
        .observed(AgentId::Codex, key)
        .and_then(|profile| profile.login.as_ref())
        .and_then(|login| CodexLogin::of(login).auth_claims().ok().flatten()))
}

#[cfg(test)]
pub(crate) mod tests;
