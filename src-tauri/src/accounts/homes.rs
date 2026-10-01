use super::{
    model::Identity,
    store::{Database, Guard, Login, Profile, Store, Ticket},
    transaction::NativeGuard,
    usage_renew::Renewals,
    Home,
};
use crate::dto::AgentId;
use std::path::{Path, PathBuf};

pub(super) type Resolve<'a> = dyn Fn(&str) -> Result<Box<dyn Home>, String> + 'a;

fn root(home: &Path) -> PathBuf {
    home.join(".on-n-off/accounts/homes")
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
    let locks = home.lock()?;
    let live = home.read()?;
    let own = match &live {
        Some(login) => home.identify(login)? == profile.identity,
        None => false,
    };
    match (live, &profile.login) {
        (Some(login), None) if own => {
            profile.login = Some(login);
            Ok(Some(CheckedOut { home, locks }))
        }
        (Some(_), Some(_)) if own => Ok(Some(CheckedOut { home, locks })),
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
        let _ = take_back_or_tear_down(id, &db, &ticket, open, home);
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
    let locks = home.lock()?;
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
    db: &Database,
    ticket: &Ticket,
    open: &dyn Fn() -> Result<Store, String>,
    home: &Resolve<'_>,
) -> Result<(), String> {
    let home = home(id)?;
    let locks = home.lock()?;
    let owner = home.read()?.and_then(|login| {
        let identity = home.identify(&login).ok()?;
        db.profiles
            .iter()
            .find(|p| p.identity == identity && p.needs_login() && !p.signed_out)
    });
    let Some(owner) = owner else {
        return home.delete(locks);
    };
    drop(locks);
    open()?.publish(ticket, |db| {
        let target = held_profile(db, &owner.id)?;
        if !target.needs_login() || target.signed_out {
            return Err("The account changed while its home was taken back.".into());
        }
        target.home = Some(id.to_string());
        Ok(())
    })
}

#[cfg(test)]
mod tests;
