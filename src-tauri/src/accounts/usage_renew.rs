use super::{
    store::{Guard, Login, Profile, Sealer, Store},
    vault,
};
use crate::{dto::AgentId, file_lease::FileLease};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct Journal {
    fingerprint: String,
    login: Option<Login>,
}

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

    pub(super) fn activation_ready(&self, profile: &Profile) -> Result<(), String> {
        let Some(journal) = self.read(&self.journal(profile))? else {
            return Ok(());
        };
        let held = profile
            .login
            .as_ref()
            .map(|login| super::view(profile.identity.provider, login).map(|v| v.fingerprint()))
            .transpose()?;
        if held.is_some_and(|fingerprint| fingerprint == journal.fingerprint) {
            return Err("This login has an unfinished usage renewal. Refresh usage to recover it, or sign in again before switching.".into());
        }
        Ok(())
    }
}

impl Renewals {
    pub(super) fn finished(&self, profile: &Profile) -> Result<Option<Login>, String> {
        let Some(journal) = self.read(&self.journal(profile))? else {
            return Ok(None);
        };
        let held = profile
            .login
            .as_ref()
            .map(|login| super::view(profile.identity.provider, login).map(|v| v.fingerprint()))
            .transpose()?;
        if held.as_deref() != Some(journal.fingerprint.as_str()) {
            return Ok(None);
        }
        let renewed = journal.login.ok_or(
            "This login has an unfinished usage renewal. Sign in again to refresh this account.",
        )?;
        if super::view(profile.identity.provider, &renewed)?.identity()? != profile.identity {
            return Err("Renewal returned a different account. Sign in again.".into());
        }
        Ok(Some(renewed))
    }

    pub(super) fn forget(&self, profile: &Profile) {
        let _ = std::fs::remove_file(self.journal(profile));
    }

    #[cfg(test)]
    pub(super) fn record_finished(&self, profile: &Profile, renewed: Option<&Login>) {
        let source = profile.login.as_ref().expect("a login to renew");
        let journal = Journal {
            fingerprint: super::view(profile.identity.provider, source)
                .unwrap()
                .fingerprint(),
            login: renewed.cloned(),
        };
        std::fs::create_dir_all(&self.root).unwrap();
        let sealed = self
            .sealer
            .seal(&serde_json::to_vec(&journal).unwrap())
            .unwrap();
        vault::atomic_write(&self.journal(profile), &sealed).unwrap();
    }
}

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
    if !super::adapter(profile.identity.provider)?.renews_privately() {
        return Err("This login renews in its own home.".into());
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
    drop(store);
    let fingerprint = super::view(profile.identity.provider, source)?.fingerprint();
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
    if super::view(profile.identity.provider, &renewed)?.identity()? != profile.identity {
        return Err("Renewal returned a different account. Sign in again.".into());
    }
    open()?.publish(&ticket, |db| {
        let target = db
            .profiles
            .iter_mut()
            .find(|p| p.id == profile.id)
            .ok_or("The saved login changed during renewal.")?;
        target.login = Some(renewed.clone());
        Ok(())
    })?;
    let _ = std::fs::remove_file(path);
    Ok(renewed)
}

pub(super) fn request(provider: AgentId, login: &Login, now_ms: i64) -> Result<Login, String> {
    let adapter = super::adapter(provider)?;
    let token_url = adapter
        .token_url()
        .ok_or("This provider's saved logins are never renewed by on-n-off.")?;
    adapter.renew_private(login, now_ms, token_url)
}

pub(super) fn grant(token_url: &str, request: &Value) -> Result<Value, crate::http::HttpError> {
    crate::http::post_grant(token_url, request)
}

#[cfg(test)]
mod tests;
