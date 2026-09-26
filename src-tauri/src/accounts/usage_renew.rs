//! Renewal of app-private logins; never touches the active native store. A private file lease
//! serializes each grant. An encrypted intent survives an ambiguous response or crash, so the
//! next poll cannot replay a possibly consumed refresh token. A completed reply is encrypted
//! before vault publication and can be adopted after a failed save.
use super::{
    model,
    store::{Guard, Login, Profile, Sealer, Store},
    vault,
};
use crate::{dto::AgentId, file_lease::FileLease};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct Journal {
    fingerprint: String,
    login: Option<Login>,
}

/// Where private renewals keep their encrypted journals beside the vault, and the vault key that
/// seals them.
pub(super) struct Renewals {
    root: PathBuf,
    sealer: Sealer,
}
impl Renewals {
    pub(super) fn of(store: &Store) -> Self {
        Self {
            root: store.root.join("usage-renewals"),
            sealer: store.sealer(),
        }
    }
    /// One profile's journal, named without its id.
    fn journal(&self, profile: &Profile) -> PathBuf {
        self.root.join(format!(
            "{}.enc",
            crate::sha::sha256_hex(profile.id.as_bytes())
        ))
    }
    fn read(&self, path: &Path) -> Result<Option<Journal>, String> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Cannot read the protected renewal record.".into()),
        };
        serde_json::from_slice(&self.sealer.unseal(&bytes)?)
            .map(Some)
            .map_err(|_| "The protected renewal record is unreadable.".into())
    }

    /// Activation must not publish a source token whose renewal may already have consumed it.
    /// The provider's exclusive activity lease excludes a concurrent polling attempt here.
    pub(super) fn activation_ready(&self, profile: &Profile) -> Result<(), String> {
        let Some(journal) = self.read(&self.journal(profile))? else {
            return Ok(());
        };
        if profile
            .login
            .as_ref()
            .is_some_and(|l| l.fingerprint() == journal.fingerprint)
        {
            return Err("This login has an unfinished usage renewal. Refresh usage to recover it, or sign in again before switching.".into());
        }
        Ok(())
    }
}

/// Serializes grants for one profile across threads and app instances, without waiting.
fn acquire_renewal_lease(root: &Path, id: &str) -> Result<FileLease, String> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(format!("{id}.lock")))
        .map_err(|_| "Cannot coordinate account renewal.")?;
    FileLease::acquire(file, std::fs::File::try_lock)
        .map_err(|_| "Another account renewal is running.".into())
}

pub(super) fn renew_owned(
    profile: &Profile,
    open: &dyn Fn() -> Result<Store, String>,
    request: &dyn Fn(&Login) -> Result<Login, String>,
) -> Result<Login, String> {
    if !profile.usage_renewal_owned {
        return Err("This login is owned by a native client.".into());
    }
    let source = profile
        .login
        .as_ref()
        .ok_or("Sign in again to refresh this account.")?;
    let store = open()?;
    let ticket = store.load()?.ticket(Guard::Renewal(profile))?;
    let renewals = Renewals::of(&store);
    std::fs::create_dir_all(&renewals.root)
        .map_err(|_| "Cannot prepare protected renewal storage.")?;
    let id = crate::sha::sha256_hex(profile.id.as_bytes());
    let _lease = acquire_renewal_lease(&renewals.root, &id)?;
    let path = renewals.journal(profile);
    drop(store); // Vault publication lock never spans a provider request.
    let fingerprint = source.fingerprint();
    let previous = renewals.read(&path)?;
    let save = |journal: &Journal| -> Result<(), String> {
        let bytes = serde_json::to_vec(journal).map_err(|_| "Cannot encode protected renewal.")?;
        vault::atomic_write(&path, &renewals.sealer.seal(&bytes)?)
    };
    let renewed = if let Some(journal) = previous.filter(|j| j.fingerprint == fingerprint) {
        journal
            .login
            .ok_or("A prior renewal did not finish. Sign in again to refresh this account.")?
    } else {
        save(&Journal {
            fingerprint: fingerprint.clone(),
            login: None,
        })?;
        let renewed = request(source)?;
        save(&Journal {
            fingerprint,
            login: Some(renewed.clone()),
        })?;
        renewed
    };
    if model::identity(profile.identity.provider, &renewed.auth, &renewed.account)?
        != profile.identity
    {
        return Err("Renewal returned a different account. Sign in again.".into());
    }
    // Only this profile's login and ownership vouch for the renewed login, never the epoch.
    open()?.publish(&ticket, |db| {
        // `publish` checked the ticket against this same database under this lease, so the profile
        // is saved and this cannot fail today. It fails closed all the same: a publication that
        // wrote nothing but succeeded would delete the journal below, the only copy of a login
        // whose refresh token is already spent.
        let target = db
            .profiles
            .iter_mut()
            .find(|p| p.id == profile.id)
            .ok_or("The saved login changed during renewal.")?;
        target.login = Some(renewed.clone());
        Ok(())
    })?;
    // A leftover completed journal is harmless: its source fingerprint no longer matches.
    let _ = std::fs::remove_file(path);
    Ok(renewed)
}

pub(super) fn request(provider: AgentId, login: &Login, now_ms: i64) -> Result<Login, String> {
    request_at(
        provider,
        login,
        now_ms,
        super::claude_renew::TOKEN_URL,
        "https://auth.openai.com/oauth/token",
    )
}
fn request_at(
    provider: AgentId,
    login: &Login,
    now_ms: i64,
    claude_url: &str,
    codex_url: &str,
) -> Result<Login, String> {
    let mut login = login.clone();
    match provider {
        AgentId::Claude => {
            login.auth = super::claude_renew::renew_private(&login.auth, now_ms, claude_url)?;
        }
        AgentId::Codex => {
            // Codex login/src/oauth/client.rs uses a JSON ChatGPT refresh grant. Native
            // credentials still renew exclusively through the official app-server path.
            let token = model::string(&login.auth, "/tokens/refresh_token")?;
            let reply=crate::http::post_grant(codex_url,&json!({"grant_type":"refresh_token","refresh_token":token,"client_id":"app_EMoamEEZ73f0CkXaXp7hrann"}))
                .map_err(|_|"Could not renew the private Codex login. Sign in again if needed.")?;
            let access = model::string(&reply, "/access_token")?.to_owned();
            login.auth["tokens"]["access_token"] = Value::String(access);
            for name in ["refresh_token", "id_token"] {
                if let Some(value) = reply
                    .get(name)
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                {
                    login.auth["tokens"][name] = Value::String(value.to_owned());
                }
            }
            login.auth["last_refresh"] = Value::String(
                chrono::DateTime::from_timestamp_millis(now_ms)
                    .ok_or("Invalid renewal time.")?
                    .to_rfc3339(),
            );
        }
        _ => return Err("This provider does not support private renewal.".into()),
    }
    Ok(login)
}

#[cfg(test)]
mod tests;
