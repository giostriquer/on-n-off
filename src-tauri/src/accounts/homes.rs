//! Where a saved account's login waits while it is not the signed-in one: its home, a private store
//! of the provider's own client ([`Home`]), where that client renews the login whenever on-n-off
//! asks it for the account's usage. on-n-off sends no request with that login and keeps no copy.
//!
//! Every login lives in exactly one store at a time: the provider's own, for the signed-in account,
//! or the account's home. The issuer replaces a refresh token each time it renews one, so a second
//! copy stops working the first time the other is renewed, and switching to it would sign the user
//! out. Two moves keep the one copy, each under the home's own locks:
//!
//! - **Checking out**, before a switch ([`check_out`]): the home's login comes into the vault and the
//!   home is emptied. The switch then publishes that login as it publishes any saved one.
//! - **Checking in**, before every read of saved accounts ([`settle`]): each saved account that is
//!   not the signed-in one and has a login in the vault gets it moved into its home.
//!
//! A crash between the halves of either move leaves the one login in both: nothing renews a home
//! while its profile still holds a vault login, since only a profile without one is read from its
//! home. When the two differ, the vault's was put there since the home got its copy, by a switch
//! away from the account or a save of it, from the native store, which is authoritative: it is the
//! newer, and it is the one that stays.
//!
//! A home no profile names, because its account was removed, signed in again or signed out, goes
//! at the next read. Its id is recorded in the vault before anything is written there, so a home
//! found on disk before the vault is read and not named in it is one nothing will use again.
use super::{
    model::Identity,
    store::{Database, Guard, Login, Profile, Store, Ticket},
    transaction::NativeGuard,
    usage_renew::Renewals,
    Home,
};
use crate::dto::AgentId;
use std::path::{Path, PathBuf};

/// How a saved account's home is found from its id.
pub(super) type Resolve<'a> = dyn Fn(&str) -> Result<Box<dyn Home>, String> + 'a;

/// Where the homes under `home` are.
fn root(home: &Path) -> PathBuf {
    home.join(".on-n-off/accounts/homes")
}

/// The directory of the home `id` names under `home`. The id comes from the vault, so one that is
/// not a home id on-n-off made is refused rather than joined onto a path.
pub(super) fn dir(home: &Path, id: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid saved account home.")?;
    Ok(root(home).join(id))
}

/// A switch's target, checked out of its home: the home, emptied by [`CheckedOut::empty`] once the
/// vault holding the login is durable, and its locks until then.
pub(super) struct CheckedOut {
    home: Box<dyn Home>,
    locks: Box<dyn NativeGuard>,
}

impl CheckedOut {
    /// Empties the home, so the switch publishes the one copy of the login.
    pub(super) fn empty(self) -> Result<(), String> {
        self.home.clear(self.locks.as_ref())
    }
}

/// Brings saved profile `id`'s login out of its home into `db`, for a switch to it, with the home
/// `home` resolves for an id. `None` when the vault's login is the one to publish: the profile has
/// no home, an earlier check-out emptied it, the home holds another account's login, or the vault's
/// is newer than the home's. The caller persists `db`, then empties the home, before anything
/// publishes the login.
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
        (Some(login), saved) if own && saved.as_ref().is_none_or(|v| v.auth == login.auth) => {
            profile.login = Some(login);
            Ok(Some(CheckedOut { home, locks }))
        }
        (_, Some(_)) => Ok(None),
        (Some(_), None) => {
            Err("This account's saved login signs in as a different account. Sign in again.".into())
        }
        (None, None) => Err("This account's saved login has ended. Sign in again.".into()),
    }
}

/// Before a read of saved `provider` accounts: tears down every home no profile names, then moves
/// the login of every saved account that is not `native`, the signed-in one, and is not archived,
/// from the vault into its home. `open` opens the vault; `home` resolves a home id. Best effort:
/// whatever fails stays as it is, and the next read tries again.
pub(super) fn settle(
    root_home: &Path,
    provider: AgentId,
    native: Option<&Identity>,
    open: &dyn Fn() -> Result<Store, String>,
    home: &Resolve<'_>,
) {
    // Listed before the vault is read: a home made after this is not swept, and one named in the
    // vault when it is read is kept.
    let found = on_disk(root_home);
    let Ok(store) = open() else {
        return;
    };
    let renewals = Renewals::of(&store);
    let Ok(db) = store.load() else {
        return;
    };
    drop(store);
    // A pending recovery refuses the ticket: nothing moves or goes until the interrupted switch
    // recovers.
    let Ok(ticket) = db.ticket(Guard::SignIn) else {
        return;
    };
    for id in found
        .iter()
        .filter(|id| !db.profiles.iter().any(|p| p.home.as_ref() == Some(*id)))
    {
        let _ = tear_down(id, home);
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

/// The ids of the homes on disk under `home`.
fn on_disk(home: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root(home)) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| uuid::Uuid::parse_str(name).is_ok())
        .collect()
}

/// Moves `profile`'s vault login into its home, first giving it one if it has none. A login a
/// private renewal left finished but unpublished moves in its place, and its record goes.
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
        // Recorded before anything is written there, so no home holds a login the vault does not
        // name. Another read that got here first named one already: that one it is.
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
        // An earlier move put the login here before it could leave the vault.
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

/// Puts `login` in `home` under `locks`, and makes sure it is what the home now holds, before the
/// vault lets go of its copy.
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

/// Tears down home `id`, one no profile names, under its locks: what its client filed outside it,
/// then the directory. A home whose client holds its locks is left for the next read.
fn tear_down(id: &str, home: &Resolve<'_>) -> Result<(), String> {
    let home = home(id)?;
    let locks = home.lock()?;
    home.delete(locks)
}

#[cfg(test)]
mod tests;
