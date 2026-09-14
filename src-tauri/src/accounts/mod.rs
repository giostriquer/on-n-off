//! Opt-in saved native logins. Native stores own active generations; the vault never refreshes.
pub(crate) mod model;
pub(crate) mod vault;

mod store;

mod transaction;

pub(crate) mod native;

pub(crate) mod activity;
mod clients;

use crate::dto::AgentId;
use serde::Serialize;
use std::path::PathBuf;
use transaction::Native;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDto {
    id: String,
    observation_id: String,
    identity: model::Identity,
    label: String,
    email: Option<String>,
    category: Option<String>,
    saved_at: String,
    active: bool,
    needs_login: bool,
    pending_activation: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsDto {
    profiles: Vec<ProfileDto>,
    native_account: Option<model::Identity>,
    native_observation_id: Option<String>,
    recovery_required: bool,
    notice: Option<String>,
    remember_accounts: bool,
}
fn home() -> Result<PathBuf, String> {
    crate::paths::user_home().map_err(|_| "Cannot resolve account storage home.".into())
}
pub fn list(provider: AgentId) -> Result<AccountsDto, String> {
    let _read =
        activity::read(provider).ok_or("An account change is running. Retry when it finishes.")?;
    let home = home()?;
    login::recover_abandoned(&home);
    let native = native::NativeStore::resolve(provider, &home)?;
    let current = native
        .read()
        .and_then(|v| v.map(|l| native.identify(&l)).transpose());
    let (current, notice) = match current {
        Ok(v) => (v, None),
        Err(e) => (None, Some(e)),
    };
    let db = if home.join(".on-n-off/accounts/vault.enc").exists() {
        store::Store::open_read(&home)?.load()?
    } else {
        store::Database::default()
    };
    let recovery_required = db
        .recovery
        .as_ref()
        .and_then(|journal| db.profiles.iter().find(|p| p.id == journal.target_id))
        .is_some_and(|profile| profile.identity.provider == provider);
    Ok(AccountsDto {
        profiles: db
            .profiles
            .into_iter()
            .filter(|p| p.identity.provider == provider)
            .map(|p| ProfileDto {
                observation_id: p.identity.observation_key(),
                active: current.as_ref() == Some(&p.identity),
                id: p.id,
                identity: p.identity,
                label: p.label,
                email: p.email,
                category: p.category,
                saved_at: p.saved_at,
                needs_login: p.login.is_none(),
                pending_activation: p.pending_activation,
            })
            .collect(),
        remember_accounts: discovery::enabled(&home).unwrap_or(false),
        native_observation_id: current.as_ref().map(model::Identity::observation_key),
        native_account: current,
        recovery_required,
        notice: notice.or_else(|| discovery::notice(provider)),
    })
}
/// Explicitly retry vault authorization without changing profiles or the native CLI login.
pub fn unlock() -> Result<(), String> {
    let home = home()?;
    if !home.join(".on-n-off/accounts/vault.enc").exists() {
        return Ok(());
    }
    store::Store::open(&home, false)?.load().map(|_| ())
}

pub fn save_current(provider: AgentId) -> Result<(), String> {
    let _read = activity::read(provider).ok_or("An account change is running.")?;
    let home = home()?;
    let native = native::NativeStore::resolve(provider, &home)?;
    native.preflight()?;
    native.verify()?;
    let store = store::Store::open(&home, true)?;
    let mut db = store.load()?;
    let native_locks = native.lock()?;
    let login = native
        .read()?
        .ok_or("Sign in with the official CLI before saving this account.")?;
    let identity = native.identify(&login)?;
    if db.recovery.is_some() {
        return Err("Recover the interrupted account change first.".into());
    }
    if db
        .profiles
        .iter()
        .any(|p| p.identity == identity && p.pending_activation)
    {
        return Err("This account has a new saved sign-in awaiting activation. Use it before saving the current login.".into());
    }
    db.invalidate_logins()?;
    db.reenroll(&identity);
    db.save(identity, login, None)?;
    store.persist(&db)?;
    drop(native_locks);
    drop(store);
    crate::read_revision::announce(crate::read_revision::Source::Accounts);
    Ok(())
}
pub fn set_category(id: &str, category: &str) -> Result<(), String> {
    let store = store::Store::open(&home()?, false)?;
    let mut db = store.load()?;
    db.set_category(id, category)?;
    store.persist(&db)?;
    drop(store);
    crate::read_revision::announce(crate::read_revision::Source::Accounts);
    Ok(())
}
pub fn remove(id: &str) -> Result<(), String> {
    login::cancel_expected(id);
    let store = store::Store::open(&home()?, false)?;
    let mut db = store.load()?;
    if db.recovery.is_some() {
        return Err("Recover the interrupted account change before removing profiles.".into());
    }
    db.invalidate_logins()?;
    if let Some(profile) = db.profiles.iter().find(|p| p.id == id) {
        if !db.ignored_accounts.contains(&profile.identity) {
            db.ignored_accounts.push(profile.identity.clone());
        }
    }
    db.profiles.retain(|p| p.id != id);
    store.persist(&db)?;
    drop(store);
    crate::read_revision::announce(crate::read_revision::Source::Accounts);
    Ok(())
}
pub fn use_profile(provider: AgentId, id: &str, recover: bool) -> Result<(), String> {
    let change = activity::change(provider)?;
    let home = home()?;
    let native = native::NativeStore::resolve(provider, &home)?;
    native.preflight()?;
    if recover {
        clients::require_closed(provider)?;
    } else {
        clients::require_activation_safe(provider)?;
    }
    let store = store::Store::open(&home, false)?;
    let mut db = store.load()?;
    if recover {
        let journal = db.recovery.as_ref().ok_or("No recovery is pending.")?;
        let target = db
            .profiles
            .iter()
            .find(|p| p.id == journal.target_id)
            .ok_or("Recovery profile is missing.")?;
        if target.identity.provider != provider {
            return Err("Recovery belongs to the other provider.".into());
        }
    } else if !db
        .profiles
        .iter()
        .any(|p| p.id == id && p.identity.provider == provider)
    {
        return Err("Profile does not belong to this provider.".into());
    }
    db.invalidate_logins()?;
    store.persist(&db)?;
    let result = if recover {
        transaction::recover(&mut db, &native, &mut |db| store.persist(db))
    } else {
        transaction::activate(&mut db, &native, id, &mut |db| store.persist(db))
    };
    drop(store);
    drop(change);
    changed(provider);
    result
}
pub fn sign_out(provider: AgentId) -> Result<(), String> {
    let change = activity::change(provider)?;
    let home = home()?;
    let native = native::NativeStore::resolve(provider, &home)?;
    native.preflight()?;
    clients::require_closed(provider)?;
    let current = native.read()?.ok_or("No native account is signed in.")?;
    let identity = native.identify(&current)?;
    let store = store::Store::open(&home, true)?;
    let mut db = store.load()?;
    if db.recovery.is_some() {
        return Err("Recover the interrupted account change before signing out.".into());
    }
    db.invalidate_logins()?;
    let fingerprint = current.fingerprint();
    if !db.ignored_credentials.contains(&fingerprint) {
        db.ignored_credentials.push(fingerprint);
    }
    // Logout may revoke shared refresh lineage. Invalidate every saved workspace for this user first.
    for profile in &mut db.profiles {
        if profile.identity.provider == provider && profile.identity.user_id == identity.user_id {
            profile.login = None;
        }
    }
    store.persist(&db)?;
    let result = native.logout().and_then(|()| {
        if native.read()?.is_none() {
            Ok(())
        } else {
            Err("The CLI still reports a login after sign-out.".into())
        }
    });
    drop(store);
    drop(change);
    changed(provider);
    result
}
fn changed(provider: AgentId) {
    crate::subscription::browser::invalidate_identity();
    crate::limits_refresh::account_changed(provider);
    crate::read_revision::announce(crate::read_revision::Source::Accounts);
}

pub(crate) mod claude_renew;

mod login;
pub use login::{add, cancel};

pub(crate) mod discovery;

/// Keep the account-operation lease through the consumer's final identity-bound write.
/// Billing receives only identity metadata, never saved OAuth credentials.
pub(crate) fn with_saved_billing_identity<T>(
    home: &std::path::Path,
    key: &str,
    consume: impl FnOnce(Result<Option<model::Identity>, String>) -> T,
) -> T {
    if !key.starts_with("profile:") || !home.join(".on-n-off/accounts/vault.enc").exists() {
        return consume(Ok(None));
    }
    match store::Store::open_read(home) {
        Ok(store) => store.with_billing_identity(key, consume),
        Err(error) => consume(Err(error)),
    }
}
