use super::{
    login_state::SignInEnd,
    model::Identity,
    store::{Database, Guard, Login, Profile, Store, Ticket},
    transaction::NativeGuard,
    usage_renew::Renewals,
    Home,
};
use crate::dto::AgentId;
use std::path::{Path, PathBuf};

pub(super) type Resolve<'a> = dyn Fn(&str) -> Result<Box<dyn Home>, String> + 'a;
pub(super) type Layouts<'a> = dyn Fn(AgentId, &Path) -> bool + 'a;

const RECORD: &str = "provider.json";
const NOT_ITS_OWN: &str = "This account's home belongs to another provider, so it was left alone.";

fn root(home: &Path) -> PathBuf {
    home.join(".on-n-off/accounts/homes")
}

pub(super) enum Claim<'a> {
    Named(AgentId),
    Unnamed(&'a Layouts<'a>),
}

enum Record {
    Absent,
    Of(AgentId),
    Unreadable,
}

fn recorded(dir: &Path) -> Record {
    match std::fs::read(dir.join(RECORD)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Record::Absent,
        Ok(bytes) if bytes.len() < 100 => {
            serde_json::from_slice(&bytes).map_or(Record::Unreadable, Record::Of)
        }
        _ => Record::Unreadable,
    }
}

pub(super) fn recorded_provider(dir: &Path) -> Option<AgentId> {
    match recorded(dir) {
        Record::Of(provider) => Some(provider),
        Record::Absent | Record::Unreadable => None,
    }
}

pub(super) fn record(dir: &Path, provider: AgentId) -> Result<(), String> {
    match recorded(dir) {
        Record::Of(owner) if owner == provider => Ok(()),
        Record::Absent => super::vault::atomic_write(
            &dir.join(RECORD),
            &serde_json::to_vec(&provider)
                .map_err(|_| "Cannot record the provider this home belongs to.")?,
        ),
        Record::Of(_) | Record::Unreadable => Err(NOT_ITS_OWN.into()),
    }
}

pub(super) fn owner(dir: &Path, claim: Claim<'_>) -> Option<AgentId> {
    match (recorded(dir), claim) {
        (Record::Of(provider), _) | (Record::Absent, Claim::Named(provider)) => Some(provider),
        (Record::Unreadable, _) => None,
        (Record::Absent, Claim::Unnamed(laid_out)) => {
            let mut found = super::PROVIDERS
                .into_iter()
                .filter(|&provider| laid_out(provider, dir));
            match (found.next(), found.next()) {
                (Some(provider), None) => Some(provider),
                _ => None,
            }
        }
    }
}

fn owned(home: &dyn Home, provider: AgentId, claim: Claim<'_>) -> Result<(), String> {
    if owner(home.dir(), claim) == Some(provider) {
        Ok(())
    } else {
        Err(NOT_ITS_OWN.into())
    }
}

pub(super) fn retire(home: &dyn Home, provider: AgentId) -> Result<(), String> {
    owned(home, provider, Claim::Named(provider))?;
    let locks = home.lock()?;
    home.delete(locks)
}

pub(super) fn remove(
    dir: &Path,
    locks: Box<dyn super::transaction::NativeGuard>,
) -> Result<(), String> {
    let removed = match std::fs::remove_dir_all(dir) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err("Cannot remove the saved account's home.".into())
        }
        _ => Ok(()),
    };
    drop(locks);
    removed
}

pub(super) fn dir(home: &Path, id: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid saved account home.")?;
    Ok(root(home).join(id))
}

pub(super) struct CheckedOut {
    home: Box<dyn Home>,
    locks: Box<dyn NativeGuard>,
}

impl CheckedOut {
    pub(super) fn empty(self) -> Result<(), String> {
        self.home.clear(self.locks.as_ref())
    }
}

pub(super) fn check_out(
    db: &mut Database,
    id: &str,
    home: &Resolve<'_>,
) -> Result<Option<CheckedOut>, String> {
    let profile = db
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or("Saved profile no longer exists.")?;
    let Some(home) = profile.home.as_deref().map(home).transpose()? else {
        return Ok(None);
    };
    let provider = profile.identity.provider;
    owned(home.as_ref(), provider, Claim::Named(provider))?;
    let locks = home.lock()?;
    let live = home.read()?;
    let own = match &live {
        Some(login) => home.identify(login)? == profile.identity,
        None => false,
    };
    match (live, &profile.login) {
        (Some(login), None) if own => {
            profile.sign_in_end = SignInEnd::of(&profile.identity, &login);
            profile.login = Some(login);
            Ok(Some(CheckedOut { home, locks }))
        }
        (Some(_), Some(saved)) if own => {
            profile.sign_in_end = SignInEnd::of(&profile.identity, saved);
            Ok(Some(CheckedOut { home, locks }))
        }
        (_, Some(_)) => Ok(None),
        (Some(_), None) => {
            Err("This account's saved login signs in as a different account. Sign in again.".into())
        }
        (None, None) => Err("This account's saved login has ended. Sign in again.".into()),
    }
}

pub(super) fn settle(
    root_home: &Path,
    provider: AgentId,
    native: Option<&Identity>,
    open: &dyn Fn() -> Result<Store, String>,
    home: &Resolve<'_>,
    laid_out: &Layouts<'_>,
) {
    let found = on_disk(root_home);
    let Ok(store) = open() else {
        return;
    };
    let renewals = Renewals::of(&store);
    let Ok(db) = store.load() else {
        return;
    };
    drop(store);
    let Ok(ticket) = db.ticket(Guard::SignIn) else {
        return;
    };
    for id in found
        .iter()
        .filter(|id| !db.profiles.iter().any(|p| p.home.as_ref() == Some(*id)))
    {
        let Ok(unnamed) = home(id) else {
            continue;
        };
        if owned(unnamed.as_ref(), provider, Claim::Unnamed(laid_out)).is_ok() {
            let _ = take_back_or_tear_down(id, unnamed.as_ref(), &db, &ticket, open);
        }
    }
    let archived = crate::limits::archived(root_home, provider);
    for profile in db.profiles.iter().filter(|p| {
        p.identity.provider == provider
            && p.login.is_some()
            && native != Some(&p.identity)
            && !archived.contains(&p.identity.observation_key())
    }) {
        let _ = check_in(profile, &ticket, &renewals, open, home);
    }
}

fn on_disk(home: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root(home)) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| uuid::Uuid::parse_str(name).is_ok())
        .collect()
}

fn check_in(
    profile: &Profile,
    ticket: &Ticket,
    renewals: &Renewals,
    open: &dyn Fn() -> Result<Store, String>,
    home: &Resolve<'_>,
) -> Result<(), String> {
    let saved = profile.login.as_ref().ok_or("Nothing to move.")?;
    let login = renewals.finished(profile)?.unwrap_or_else(|| saved.clone());
    let fingerprint = super::view(profile.identity.provider, saved)?.fingerprint();
    let held = ticket.holding(profile, fingerprint);
    let id = match &profile.home {
        Some(id) => id.clone(),
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            open()?.publish(&held, |db| {
                let target = held_profile(db, &profile.id)?;
                Ok(target.home.get_or_insert(id).clone())
            })?
        }
    };
    let home = home(&id)?;
    let provider = profile.identity.provider;
    owned(home.as_ref(), provider, Claim::Named(provider))?;
    let locks = home.lock()?;
    record(home.dir(), provider)?;
    match home.read()? {
        Some(live) if home.identify(&live)? != profile.identity => {
            return Err("This account's home holds another account's login.".into())
        }
        Some(live) if live.auth == login.auth => {}
        _ => put(home.as_ref(), &login, locks.as_ref())?,
    }
    drop(locks);
    open()?.publish(&held, |db| {
        let target = held_profile(db, &profile.id)?;
        if target.home.as_deref() == Some(id.as_str()) {
            target.sign_in_end = SignInEnd::of(&target.identity, &login);
            target.login = None;
        }
        Ok(())
    })?;
    renewals.forget(profile);
    Ok(())
}

fn put(home: &dyn Home, login: &Login, locks: &dyn NativeGuard) -> Result<(), String> {
    let back = home.put(login, locks)?;
    if back.as_ref().map(|back| &back.auth) != Some(&login.auth) {
        return Err("The login could not be read back from its home.".into());
    }
    Ok(())
}

fn held_profile<'db>(db: &'db mut Database, id: &str) -> Result<&'db mut Profile, String> {
    db.profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| "Saved profile no longer exists.".into())
}

fn take_back_or_tear_down(
    id: &str,
    home: &dyn Home,
    db: &Database,
    ticket: &Ticket,
    open: &dyn Fn() -> Result<Store, String>,
) -> Result<(), String> {
    let locks = home.lock()?;
    let account = home.read()?.and_then(|login| {
        let identity = home.identify(&login).ok()?;
        let account = db
            .profiles
            .iter()
            .find(|p| p.identity == identity && p.needs_login() && !p.signed_out)?;
        Some((account, SignInEnd::of(&identity, &login)))
    });
    let Some((account, sign_in_end)) = account else {
        return home.delete(locks);
    };
    record(home.dir(), account.identity.provider)?;
    drop(locks);
    open()?.publish(ticket, |db| {
        let target = held_profile(db, &account.id)?;
        if !target.needs_login() || target.signed_out {
            return Err("The account changed while its home was taken back.".into());
        }
        target.home = Some(id.to_string());
        target.sign_in_end = sign_in_end;
        Ok(())
    })
}

#[cfg(test)]
mod tests;
