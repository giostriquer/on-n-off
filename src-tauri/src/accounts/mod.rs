//! Opt-in saved logins. Native stores own shared generations; only never-activated private
//! sign-ins can renew in the vault. Saved usage reads never activate an account.
pub(crate) mod model;
pub(crate) mod vault;

mod store;
#[cfg(test)]
pub(crate) use store::lease_timeout_override::set as override_lease_timeout;
pub(crate) mod usage;
mod usage_renew;

mod transaction;

pub(crate) mod claude_store;
mod keychain;
#[cfg(all(target_os = "macos", test))]
pub(crate) use keychain::with_real_keychain;
pub(crate) mod native;

pub(crate) mod activity;
mod clients;
pub use clients::activation_blockers;

use crate::dto::AgentId;
use serde::Serialize;
use std::path::{Path, PathBuf};
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

/// A provider's native store as the account operations use it: the activation transaction's
/// `Native`, plus the check an operation makes before touching it and the official sign-out.
trait NativeAccount: Native {
    /// Refuses a native setup on-n-off leaves to the official client: a custom home, an
    /// environment credential or a managed login policy.
    fn preflight(&self) -> Result<(), String>;
    /// Signs the CLI out through its own command.
    fn logout(&self) -> Result<(), String>;
}

/// Running provider clients, which an account change checks for before it touches a native login.
trait Clients {
    /// Refuses while clients that cannot take a native credential change are running.
    fn activation_safe(&self, provider: AgentId) -> Result<(), String>;
    /// Refuses while any of the provider's clients is running.
    fn closed(&self, provider: AgentId) -> Result<(), String>;
}

/// Who hears about an account change, once the change has released its leases.
trait Notify {
    /// Which login a provider's CLI uses may have changed: Limits replaces its reading, then the
    /// account list is announced.
    fn changed(&self, provider: AgentId);
    /// Only the saved profiles changed: the account list is announced.
    fn accounts(&self);
}

type ResolveNative = Box<dyn Fn(AgentId, &Path) -> Result<Box<dyn NativeAccount>, String>>;

/// The account operations over one home, with what they reach outside the vault: the provider's
/// native store, its running clients, and whoever hears about a change. `live` builds it from the
/// real ones; tests build it from a scratch home and fakes.
struct Accounts {
    home: PathBuf,
    native: ResolveNative,
    clients: Box<dyn Clients>,
    notify: Box<dyn Notify>,
}

struct RunningClients;
impl Clients for RunningClients {
    fn activation_safe(&self, provider: AgentId) -> Result<(), String> {
        clients::require_activation_safe(provider)
    }
    fn closed(&self, provider: AgentId) -> Result<(), String> {
        clients::require_closed(provider)
    }
}

struct Announce;
impl Notify for Announce {
    fn changed(&self, provider: AgentId) {
        changed(provider);
    }
    fn accounts(&self) {
        crate::read_revision::announce(crate::read_revision::Source::Accounts);
    }
}

impl Accounts {
    fn live() -> Result<Self, String> {
        Ok(Self {
            home: home()?,
            native: Box::new(|provider, home| {
                Ok(Box::new(native::NativeStore::resolve(provider, home)?))
            }),
            clients: Box::new(RunningClients),
            notify: Box::new(Announce),
        })
    }
    fn native(&self, provider: AgentId) -> Result<Box<dyn NativeAccount>, String> {
        (self.native)(provider, &self.home)
    }
}

pub fn list(provider: AgentId) -> Result<AccountsDto, String> {
    Accounts::live()?.list(provider)
}
/// Explicitly retry vault authorization without changing profiles or the native CLI login.
pub fn unlock() -> Result<(), String> {
    let home = home()?;
    if !store::Store::vault_exists(&home) {
        return Ok(());
    }
    store::Store::open(&home, false)?.load().map(|_| ())
}
pub fn save_current(provider: AgentId) -> Result<(), String> {
    Accounts::live()?.save_current(provider)
}
pub fn set_category(id: &str, category: &str) -> Result<(), String> {
    Accounts::live()?.set_category(id, category)
}
pub fn remove(id: &str) -> Result<(), String> {
    Accounts::live()?.remove(id)
}
pub fn use_profile(provider: AgentId, id: &str, activation: Activation) -> Result<(), String> {
    Accounts::live()?.use_profile(provider, id, activation)
}
pub fn sign_out(provider: AgentId) -> Result<(), String> {
    Accounts::live()?.sign_out(provider)
}

impl Accounts {
    fn list(&self, provider: AgentId) -> Result<AccountsDto, String> {
        let _read = activity::read(provider)
            .ok_or("An account change is running. Retry when it finishes.")?;
        let home = &self.home;
        login::recover_abandoned(home);
        let native = self.native(provider)?;
        let current = native
            .read()
            .and_then(|v| v.map(|l| native.identify(&l)).transpose());
        let (current, notice) = match current {
            Ok(v) => (v, None),
            Err(e) => (None, Some(e)),
        };
        let db = if store::Store::vault_exists(home) {
            store::Store::open_read(home)?.load()?
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
            remember_accounts: discovery::enabled(home).unwrap_or(false),
            native_observation_id: current.as_ref().map(model::Identity::observation_key),
            native_account: current,
            recovery_required,
            notice: notice.or_else(|| discovery::notice(provider)),
        })
    }

    fn save_current(&self, provider: AgentId) -> Result<(), String> {
        let _read = activity::read(provider).ok_or("An account change is running.")?;
        let native = self.native(provider)?;
        native.preflight()?;
        native.verify()?;
        let store = store::Store::open(&self.home, true)?;
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
        self.notify.accounts();
        Ok(())
    }
    fn set_category(&self, id: &str, category: &str) -> Result<(), String> {
        let store = store::Store::open(&self.home, false)?;
        let mut db = store.load()?;
        db.set_category(id, category)?;
        store.persist(&db)?;
        drop(store);
        self.notify.accounts();
        Ok(())
    }
    fn remove(&self, id: &str) -> Result<(), String> {
        login::cancel_expected(id);
        let store = store::Store::open(&self.home, false)?;
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
        self.notify.accounts();
        Ok(())
    }
}
/// How an account change treats provider clients that are still running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// Refuse while clients that cannot take a native credential change are running.
    Ordinary,
    /// The person chose to switch beside running clients, which keep the previous account.
    AlongsideClients,
    /// Explicit crash recovery, which always requires closed clients.
    Recover,
}
impl Activation {
    /// The account action that asks for this change, if it is one.
    pub fn from_action(action: &str) -> Option<Self> {
        match action {
            "use" => Some(Self::Ordinary),
            "useAlongsideClients" => Some(Self::AlongsideClients),
            "recover" => Some(Self::Recover),
            _ => None,
        }
    }
}
fn check_clients(
    activation: Activation,
    provider: AgentId,
    activation_safe: impl FnOnce(AgentId) -> Result<(), String>,
    closed: impl FnOnce(AgentId) -> Result<(), String>,
) -> Result<(), String> {
    match activation {
        Activation::Ordinary => activation_safe(provider),
        Activation::AlongsideClients => Ok(()),
        Activation::Recover => closed(provider),
    }
}
impl Accounts {
    fn use_profile(
        &self,
        provider: AgentId,
        id: &str,
        activation: Activation,
    ) -> Result<(), String> {
        let change = activity::change(provider)?;
        let native = self.native(provider)?;
        native.preflight()?;
        check_clients(
            activation,
            provider,
            |provider| self.clients.activation_safe(provider),
            |provider| self.clients.closed(provider),
        )?;
        let store = store::Store::open(&self.home, false)?;
        let mut db = store.load()?;
        match activation {
            Activation::Recover => {
                let journal = db.recovery.as_ref().ok_or("No recovery is pending.")?;
                let target = db
                    .profiles
                    .iter()
                    .find(|p| p.id == journal.target_id)
                    .ok_or("Recovery profile is missing.")?;
                if target.identity.provider != provider {
                    return Err("Recovery belongs to the other provider.".into());
                }
            }
            _ if !db
                .profiles
                .iter()
                .any(|p| p.id == id && p.identity.provider == provider) =>
            {
                return Err("Profile does not belong to this provider.".into());
            }
            _ => {}
        }
        if activation != Activation::Recover {
            let target = db
                .profiles
                .iter()
                .find(|p| p.id == id)
                .ok_or("Profile no longer exists.")?;
            usage_renew::activation_ready(&store, target)?;
        }
        db.invalidate_logins()?;
        store.persist(&db)?;
        let persist = &mut |db: &store::Database| store.persist(db);
        let native: &dyn Native = native.as_ref();
        let result = match activation {
            Activation::Recover => transaction::recover(&mut db, native, persist),
            Activation::Ordinary => transaction::activate(&mut db, native, id, false, persist),
            Activation::AlongsideClients => {
                transaction::activate(&mut db, native, id, true, persist)
            }
        };
        drop(store);
        drop(change);
        self.notify.changed(provider);
        result
    }
    fn sign_out(&self, provider: AgentId) -> Result<(), String> {
        let change = activity::change(provider)?;
        let native = self.native(provider)?;
        native.preflight()?;
        self.clients.closed(provider)?;
        let current = native.read()?.ok_or("No native account is signed in.")?;
        let identity = native.identify(&current)?;
        let store = store::Store::open(&self.home, true)?;
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
            if profile.identity.provider == provider && profile.identity.user_id == identity.user_id
            {
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
        self.notify.changed(provider);
        result
    }
}
fn changed(provider: AgentId) {
    crate::limits_refresh::account_changed(provider);
    crate::read_revision::announce(crate::read_revision::Source::Accounts);
}

pub(crate) mod claude_renew;

mod login;
pub use login::{add, cancel};

pub(crate) mod discovery;

/// The `https://api.openai.com/auth` claims of a saved Codex profile's ID token: identity and plan
/// metadata, never its tokens. `None` for an unknown key, a profile without a login, or a device
/// with no vault; an error when the vault exists and cannot be read right now.
pub(crate) fn saved_codex_claims(
    home: &std::path::Path,
    key: &str,
) -> Result<Option<serde_json::Value>, String> {
    if !model::Identity::is_profile_key(key) || !store::Store::vault_exists(home) {
        return Ok(None);
    }
    Ok(store::Store::open_read(home)?.load()?.codex_claims(key))
}

#[cfg(test)]
pub(crate) use store::tests::saved_codex_fixture;

#[cfg(test)]
mod tests;
