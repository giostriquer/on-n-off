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
    /// Whether a running client is about to renew this login, and so rewrite it.
    fn renews_soon(&self, _login: &Login) -> bool {
        false
    }
}
/// `alongside_clients` means the person chose to switch while provider clients run. Those clients
/// take no native lock and may write at any time, so a failure then restores only bytes this
/// change wrote or replaced and never verifies a login a client may own.
pub fn activate(
    db: &mut Database,
    native: &dyn Native,
    id: &str,
    alongside_clients: bool,
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
    if alongside_clients {
        // Codex clients match a login by workspace, so one would adopt the new login mid-session;
        // and one about to renew would write its account back.
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

    // Revoke private renewal ownership before any credential can reach a native client.
    if let Some(target) = db.profiles.iter_mut().find(|p| p.id == id) {
        target.usage_renewal_owned = false;
    }
    db.recovery = Some(Recovery {
        target_id: id.into(),
        outgoing,
        outgoing_identity,
    });
    persist(db)?; // No native write before the protected journal is durable.
    if let Err(error) = settle_outgoing(db, native, &profile.identity, persist) {
        // Nothing was published, so a retained journal would only demand recovery.
        db.recovery = None;
        return Err(match persist(db) {
            Ok(()) => format!("{error} Nothing was replaced."),
            Err(_) => format!("{error} Nothing was replaced, but the interrupted change could not be cleared; recover it before another action."),
        });
    }
    let publication = native.write_locked(Some(incoming), locks.as_ref());
    // Verification refreshes whatever is on disk, so it must not run on a login a client wrote.
    // Reading back under the native locks, which Claude Code's own refresh honors, narrows that
    // window; an unreadable store counts as replaced.
    let replaced = match publication.as_ref().map(|()| native.read()) {
        Ok(Ok(Some(live))) if live.auth == incoming.auth => None,
        Ok(Ok(_)) => Some("A running client replaced the new login."),
        Ok(Err(_)) => Some("The new login could not be read back."),
        Err(_) => None,
    };
    drop(locks);
    if let Some(reason) = replaced {
        return Err(restored(reason, restore_known(db, native, persist)));
    }
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
        if alongside_clients {
            return Err(restored(&error, restore_known(db, native, persist)));
        }
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
/// Codex clients take no lock, so one left running can rotate the outgoing login after capture.
/// Re-reading once the journal is durable saves the generation it moved to and refuses to publish
/// over another account. This narrows the race; it cannot close it.
fn settle_outgoing(
    db: &mut Database,
    native: &dyn Native,
    target: &Identity,
    persist: &mut dyn FnMut(&Database) -> Result<(), String>,
) -> Result<(), String> {
    let journal = db
        .recovery
        .clone()
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
    if let Some(journal) = db.recovery.as_mut() {
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
        .recovery
        .clone()
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
    let mut locks = Some(native.lock()?);
    match native.read()? {
        Some(live) if !known(&journal, &target, &live) => {
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
        // A crash can split Claude's identity/config write from its credential write.
        // Known credential bytes establish ownership before consulting the partial identity.
        _ => native.write_locked(
            journal.outgoing.as_ref(),
            locks.as_ref().ok_or("Native lock is missing.")?.as_ref(),
        )?,
    }
    finish(db, native, journal, locks, persist)
}
/// Restores the outgoing login only over bytes this change wrote or replaced. Anything else,
/// including a login a client signed out, may belong to a running client, so the journal waits for
/// an explicit recovery with clients closed.
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
    native.write_locked(journal.outgoing.as_ref(), locks.as_ref())?;
    finish(db, native, journal, Some(locks), persist)
}
fn finish(
    db: &mut Database,
    native: &dyn Native,
    journal: Recovery,
    locks: Option<Box<dyn NativeGuard>>,
    persist: &mut dyn FnMut(&Database) -> Result<(), String>,
) -> Result<(), String> {
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
