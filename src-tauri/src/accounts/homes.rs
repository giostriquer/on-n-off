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
//! A crash between the halves of either move leaves both copies, and they are the same login:
//! nothing renews a home while its profile still holds a vault login, since only a profile without
//! one is read from its home. The next check-in settles it, the home's copy standing.
use super::{
    model::Identity,
    store::{ChangeKind, Database, Guard, Profile, Store, Ticket},
    transaction::NativeGuard,
    usage_renew::Renewals,
    Home,
};
use crate::dto::AgentId;
use std::path::{Path, PathBuf};

/// How a home is found from its id: `Ok(None)` for a provider whose saved logins keep no homes.
pub(super) type Resolve<'a> = dyn Fn(&str) -> Result<Option<Box<dyn Home>>, String> + 'a;

/// The directory of the home `id` names under `home`. The id comes from the vault, so one that is
/// not a home id on-n-off made is refused rather than joined onto a path.
pub(super) fn dir(home: &Path, id: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid saved account home.")?;
    Ok(home.join(".on-n-off/accounts/homes").join(id))
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
/// `home` resolves for an id. `None` when the login is in the vault already: the profile has no
/// home, or an earlier check-out emptied the home and kept the login here. The caller persists
/// `db`, then empties the home, before anything publishes the login.
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
    let Some(home) = profile.home.as_deref().map(home).transpose()?.flatten() else {
        return Ok(None);
    };
    let locks = home.lock()?;
    match home.read()? {
        Some(login) => {
            if home.identify(&login)? != profile.identity {
                return Err(
                    "This account's saved login signs in as a different account. Sign in again."
                        .into(),
                );
            }
            profile.login = Some(login);
            Ok(Some(CheckedOut { home, locks }))
        }
        None if profile.login.is_some() => Ok(None),
        None => Err("This account's saved login has ended. Sign in again.".into()),
    }
}

/// Before a read of saved `provider` accounts: moves the login of every one that is not `native`,
/// the signed-in account, and is not archived, from the vault into its home, and tears down retired
/// homes. `open` opens the vault; `home` resolves a home id, `None` for a provider without homes.
/// Best effort: whatever fails stays as it is, and the next read tries again.
pub(super) fn settle(
    root: &Path,
    provider: AgentId,
    native: Option<&Identity>,
    open: &dyn Fn() -> Result<Store, String>,
    home: &Resolve<'_>,
) {
    let Ok(store) = open() else {
        return;
    };
    let renewals = Renewals::of(&store);
    let Ok(db) = store.load() else {
        return;
    };
    drop(store);
    // A pending recovery refuses the ticket: nothing moves until the interrupted switch recovers.
    let Ok(ticket) = db.ticket(Guard::SignIn) else {
        return;
    };
    for id in &db.retired_homes {
        let _ = tear_down(id, open, home);
    }
    let archived = crate::limits::archived(root, provider);
    for profile in db.profiles.iter().filter(|p| {
        p.identity.provider == provider
            && p.login.is_some()
            && native != Some(&p.identity)
            && !archived.contains(&p.identity.observation_key())
    }) {
        let _ = check_in(profile, &ticket, &renewals, open, home);
    }
}

/// Moves `profile`'s vault login into its home, first giving it one if it has none. A login a
/// private renewal left finished but unpublished moves in its place, and its journal goes.
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
            if home(&id)?.is_none() {
                return Ok(()); // This provider keeps its saved logins in the vault.
            }
            // Recorded before anything is written there, so no home holds a login the vault does
            // not name. Another read that got here first named one already: that one it is.
            open()?.publish(&held, |db| {
                let target = held_profile(db, &profile.id)?;
                Ok(target.home.get_or_insert(id).clone())
            })?
        }
    };
    let home = home(&id)?.ok_or("This provider keeps no homes.")?;
    let locks = home.lock()?;
    match home.read()? {
        // An earlier move put the login here before it could leave the vault: the home's copy
        // stands, and is the one any renewal since went to.
        Some(live) if home.identify(&live)? == profile.identity => {}
        Some(_) => return Err("This account's home holds another account's login.".into()),
        None => {
            let back = home.put(&login, locks.as_ref())?;
            if back.as_ref().map(|back| &back.auth) != Some(&login.auth) {
                return Err("The login could not be read back from its home.".into());
            }
        }
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

fn held_profile<'db>(db: &'db mut Database, id: &str) -> Result<&'db mut Profile, String> {
    db.profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| "Saved profile no longer exists.".into())
}

/// Tears down retired home `id`, if `home` resolves it, and forgets it once it is gone. A home
/// whose client holds its locks is left for the next read.
pub(super) fn tear_down(
    id: &str,
    open: &dyn Fn() -> Result<Store, String>,
    home: &Resolve<'_>,
) -> Result<(), String> {
    let Some(home) = home(id)? else {
        return Ok(());
    };
    drop(home.lock()?);
    home.delete()?;
    open()?.change(ChangeKind::Metadata, |db| {
        db.retired_homes.retain(|retired| retired != id);
        Ok(())
    })
}

#[cfg(test)]
mod tests;
