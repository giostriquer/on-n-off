use super::{
    model::Identity,
    store::{Database, Login},
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
pub struct Recovery {
    pub target_id: String,
    pub outgoing: Option<Login>,
    pub outgoing_identity: Option<Identity>,
}
pub trait NativeGuard {
    fn ensure(&self) -> Result<(), String> {
        Ok(())
    }
}
impl NativeGuard for () {}
pub type ReadBack = Result<Option<Login>, String>;
pub trait Native {
    fn lock(&self) -> Result<Box<dyn NativeGuard>, String> {
        Ok(Box::new(()))
    }
    fn write_locked(
        &self,
        login: Option<&Login>,
        guard: Box<dyn NativeGuard>,
    ) -> Result<ReadBack, String> {
        guard.ensure()?;
        self.write(login)?;
        Ok(self.read())
    }
    fn read(&self) -> Result<Option<Login>, String>;
    fn identify(&self, login: &Login) -> Result<Identity, String>;
    fn write(&self, login: Option<&Login>) -> Result<(), String>;
    fn verify(&self) -> Result<(), String>;
    fn verify_observed(&self) -> Result<(), String> {
        self.verify()
    }
    fn renews_soon(&self, _login: &Login) -> bool {
        false
    }
}
pub fn activate(
    db: &mut Database,
    native: &dyn Native,
    id: &str,
    alongside_clients: bool,
    persist: &mut dyn FnMut(&Database) -> Result<(), String>,
) -> Result<(), String> {
    if db.recovery().is_some() {
        return Err("An interrupted account change needs recovery first.".into());
    }
    let profile = db
        .profiles
        .iter()
        .find(|p| p.id == id)
        .ok_or("Saved profile no longer exists.")?
        .clone();
    let incoming = profile
        .login
        .as_ref()
        .ok_or("This profile needs sign-in again.")?;
    if native.identify(incoming)? != profile.identity {
        return Err("Saved credential identity does not match its profile.".into());
    }
    let locks = native.lock()?;
    let outgoing = native.read()?;
    let outgoing_identity = outgoing.as_ref().map(|v| native.identify(v)).transpose()?;
    if outgoing_identity.as_ref() == Some(&profile.identity) && !profile.pending_activation {
        db.save(
            profile.identity,
            outgoing.ok_or("Native login disappeared.")?,
            None,
        )?;
        return persist(db);
    }
    if alongside_clients {
        if outgoing_identity
            .as_ref()
            .is_some_and(|v| v.workspace_id == profile.identity.workspace_id)
        {
            return Err("Both accounts use the same workspace, so running clients would pick up the new login mid-session. Close provider clients to switch.".into());
        }
        if outgoing.as_ref().is_some_and(|v| native.renews_soon(v)) {
            return Err("A running client is about to renew the current login. Try again in a few minutes, or close provider clients to switch now.".into());
        }
    }
    capture(
        db,
        outgoing.as_ref(),
        outgoing_identity.as_ref(),
        &profile.identity,
    )?;

    if let Some(target) = db.profiles.iter_mut().find(|p| p.id == id) {
        target.usage_renewal_owned = false;
    }
    db.begin_recovery(Recovery {
        target_id: id.into(),
        outgoing,
        outgoing_identity,
    });
    persist(db)?;
    if let Err(error) = settle_outgoing(db, native, &profile.identity, persist) {
        db.end_recovery();
        return Err(match persist(db) {
            Ok(()) => format!("{error} Nothing was replaced."),
            Err(_) => format!("{error} Nothing was replaced, but the interrupted change could not be cleared; recover it before another action."),
        });
    }
    let publication = native.write_locked(Some(incoming), locks);
    let replaced = match &publication {
        Ok(Ok(Some(live))) if live.auth == incoming.auth => None,
        Ok(Ok(_)) => Some("A running client replaced the new login."),
        Ok(Err(_)) => Some("The new login could not be read back."),
        Err(_) => None,
    };
    if let Some(reason) = replaced {
        return Err(restored(reason, restore_known(db, native, persist)));
    }
    let result = publication
        .map(drop)
        .and_then(|()| native.verify())
        .and_then(|()| {
            let live = native
                .read()?
                .ok_or("Native login disappeared after activation.")?;
            if native.identify(&live)? != profile.identity {
                return Err("Native readback returned a different user or workspace.".into());
            }
            db.save(profile.identity, live, Some(id))?;
            Ok(())
        });
    if let Err(error) = result {
        if alongside_clients {
            return Err(restored(&error, restore_known(db, native, persist)));
        }
        recover(db, native, persist)?;
        return Err(format!(
            "{error} Recovery completed. Check the current account before retrying."
        ));
    }
    let journal = db.end_recovery();
    if let Err(error) = persist(db) {
        if let Some(journal) = journal {
            db.begin_recovery(journal);
        }
        return Err(format!("{error} Native login changed, but completion could not be recorded. Recover the interrupted change before another action."));
    }
    Ok(())
}
fn capture(
    db: &mut Database,
    login: Option<&Login>,
    identity: Option<&Identity>,
    target: &Identity,
) -> Result<(), String> {
    match (login, identity) {
        (Some(login), Some(identity)) if identity != target => {
            db.capture_native(identity.clone(), login.clone())
        }
        _ => Ok(()),
    }
}
fn settle_outgoing(
    db: &mut Database,
    native: &dyn Native,
    target: &Identity,
    persist: &mut dyn FnMut(&Database) -> Result<(), String>,
) -> Result<(), String> {
    let journal = db
        .recovery()
        .cloned()
        .ok_or("Protected recovery is missing.")?;
    let latest = native.read()?;
    if latest.as_ref().map(|v| &v.auth) == journal.outgoing.as_ref().map(|v| &v.auth) {
        return Ok(());
    }
    let identity = latest.as_ref().map(|v| native.identify(v)).transpose()?;
    if identity != journal.outgoing_identity {
        return Err("The native login changed during the switch.".into());
    }
    capture(db, latest.as_ref(), identity.as_ref(), target)?;
    if let Some(journal) = db.recovery_mut() {
        journal.outgoing = latest;
    }
    persist(db)
}
fn restored(error: &str, restore: Result<(), String>) -> String {
    match restore {
        Ok(()) => format!("{error} The previous login was restored; close provider clients before retrying."),
        Err(reason) => format!("{error} {reason} The change is pending recovery: close provider clients, then recover."),
    }
}
fn pending(db: &Database) -> Result<(Recovery, super::store::Profile), String> {
    let journal = db
        .recovery()
        .cloned()
        .ok_or("No account change needs recovery.")?;
    let target = db
        .profiles
        .iter()
        .find(|p| p.id == journal.target_id)
        .ok_or("Recovery target is missing.")?
        .clone();
    Ok((journal, target))
}
fn known(journal: &Recovery, target: &super::store::Profile, live: &Login) -> bool {
    journal
        .outgoing
        .as_ref()
        .is_some_and(|previous| previous.auth == live.auth)
        || target
            .login
            .as_ref()
            .is_some_and(|incoming| incoming.auth == live.auth)
}
pub fn recover(
    db: &mut Database,
    native: &dyn Native,
    persist: &mut dyn FnMut(&Database) -> Result<(), String>,
) -> Result<(), String> {
    let (journal, target) = pending(db)?;
    let locks = native.lock()?;
    let restored = match native.read()? {
        Some(live) if !known(&journal, &target, &live) => {
            drop(locks);
            native.verify().map_err(|_|"Could not verify the changed native credential. Protected recovery was retained; no credentials were overwritten.")?;
            let locks = native.lock()?;
            let live = native
                .read()?
                .ok_or("Native login disappeared during recovery.")?;
            let identity = native.identify(&live)?;
            if Some(&identity) == journal.outgoing_identity.as_ref() {
                db.capture_native(identity, live)?;
                let restored = native.read();
                drop(locks);
                restored
            } else if identity == target.identity {
                db.save(identity, live, Some(&target.id))?;
                persist(db)?;
                native.write_locked(journal.outgoing.as_ref(), locks)?
            } else {
                return Err("Native identity changed outside on-n-off. Protected recovery was retained; no credentials were overwritten.".into());
            }
        }
        _ => native.write_locked(journal.outgoing.as_ref(), locks)?,
    };
    finish(db, native, journal, restored, persist)
}
fn restore_known(
    db: &mut Database,
    native: &dyn Native,
    persist: &mut dyn FnMut(&Database) -> Result<(), String>,
) -> Result<(), String> {
    let (journal, target) = pending(db)?;
    let locks = native.lock()?;
    let owned = match native.read()? {
        Some(live) => known(&journal, &target, &live),
        None => journal.outgoing.is_none(),
    };
    if !owned {
        return Err("The native login changed outside on-n-off.".into());
    }
    let restored = native.write_locked(journal.outgoing.as_ref(), locks)?;
    finish(db, native, journal, restored, persist)
}
fn finish(
    db: &mut Database,
    native: &dyn Native,
    journal: Recovery,
    restored: ReadBack,
    persist: &mut dyn FnMut(&Database) -> Result<(), String>,
) -> Result<(), String> {
    let restored = restored?;
    let identity = restored.as_ref().map(|v| native.identify(v)).transpose()?;
    if identity != journal.outgoing_identity {
        return Err(
            "Recovery could not verify the previous login. The protected journal was retained."
                .into(),
        );
    }
    db.end_recovery();
    if let Err(error) = persist(db) {
        db.begin_recovery(journal);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
