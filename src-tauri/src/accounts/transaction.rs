//! Crash-visible activation journal; credential and identity writes are one recoverable operation.
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
pub trait Native {
    fn lock(&self) -> Result<Box<dyn NativeGuard>, String> {
        Ok(Box::new(()))
    }
    fn write_locked(&self, login: Option<&Login>, guard: &dyn NativeGuard) -> Result<(), String> {
        guard.ensure()?;
        self.write(login)
    }
    fn read(&self) -> Result<Option<Login>, String>;
    fn identify(&self, login: &Login) -> Result<Identity, String>;
    fn write(&self, login: Option<&Login>) -> Result<(), String>;
    fn verify(&self) -> Result<(), String>;
    /// Observation must not force token rotation solely to verify an unchanged login.
    fn verify_observed(&self) -> Result<(), String> {
        self.verify()
    }
}
pub fn activate(
    db: &mut Database,
    native: &dyn Native,
    id: &str,
    persist: &mut dyn FnMut(&Database) -> Result<(), String>,
) -> Result<(), String> {
    if db.recovery.is_some() {
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
    if let (Some(login), Some(identity)) = (&outgoing, &outgoing_identity) {
        if identity != &profile.identity {
            db.capture_native(identity.clone(), login.clone())?;
        }
    }

    db.recovery = Some(Recovery {
        target_id: id.into(),
        outgoing,
        outgoing_identity,
    });
    persist(db)?; // No native write before the protected journal is durable.
    let publication = native.write_locked(Some(incoming), locks.as_ref());
    drop(locks);
    let result = publication.and_then(|()| native.verify()).and_then(|()| {
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
        recover(db, native, persist)?;
        return Err(format!(
            "{error} Recovery completed. Check the current account before retrying."
        ));
    }
    let recovery = db.recovery.take();
    if let Err(error) = persist(db) {
        db.recovery = recovery;
        return Err(format!("{error} Native login changed, but completion could not be recorded. Recover the interrupted change before another action."));
    }
    Ok(())
}
pub fn recover(
    db: &mut Database,
    native: &dyn Native,
    persist: &mut dyn FnMut(&Database) -> Result<(), String>,
) -> Result<(), String> {
    let journal = db
        .recovery
        .clone()
        .ok_or("No account change needs recovery.")?;
    let target = db
        .profiles
        .iter()
        .find(|p| p.id == journal.target_id)
        .ok_or("Recovery target is missing.")?
        .clone();
    let mut locks = Some(native.lock()?);
    let live = native.read()?;
    if let Some(live) = live {
        let known_outgoing = journal
            .outgoing
            .as_ref()
            .is_some_and(|previous| previous.auth == live.auth);
        let known_incoming = target
            .login
            .as_ref()
            .is_some_and(|incoming| incoming.auth == live.auth);
        if known_outgoing || known_incoming {
            // A crash can split Claude's identity/config write from its credential write.
            // Known credential bytes establish ownership before consulting the partial identity.
            native.write_locked(
                journal.outgoing.as_ref(),
                locks.as_ref().ok_or("Native lock is missing.")?.as_ref(),
            )?;
        } else {
            // Unknown bytes may be a newly rotated generation, or an unrelated external login.
            drop(locks.take());
            native.verify().map_err(|_|"Could not verify the changed native credential. Protected recovery was retained; no credentials were overwritten.")?;
            locks = Some(native.lock()?);
            let live = native
                .read()?
                .ok_or("Native login disappeared during recovery.")?;
            let identity = native.identify(&live)?;
            if Some(&identity) == journal.outgoing_identity.as_ref() {
                db.capture_native(identity, live)?;
            } else if identity == target.identity {
                db.save(identity, live, Some(&target.id))?;
                persist(db)?;
                native.write_locked(
                    journal.outgoing.as_ref(),
                    locks.as_ref().ok_or("Native lock is missing.")?.as_ref(),
                )?;
            } else {
                return Err("Native identity changed outside on-n-off. Protected recovery was retained; no credentials were overwritten.".into());
            }
        }
    } else {
        native.write_locked(
            journal.outgoing.as_ref(),
            locks.as_ref().ok_or("Native lock is missing.")?.as_ref(),
        )?;
    }
    let restored = native.read()?;
    let identity = restored.as_ref().map(|v| native.identify(v)).transpose()?;
    if identity != journal.outgoing_identity {
        return Err(
            "Recovery could not verify the previous login. The protected journal was retained."
                .into(),
        );
    }
    drop(locks);
    db.recovery = None;
    if let Err(error) = persist(db) {
        db.recovery = Some(journal);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
