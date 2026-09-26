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
    Accounts::live()?.activate(provider, id, activation)
}
pub fn sign_out(provider: AgentId) -> Result<(), String> {
    Accounts::live()?.sign_out(provider)
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
            store::Store::open_existing(home)?.load()?
        } else {
            store::Database::default()
        };
        let recovery_required = db
            .recovery_target()
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
        let read = activity::read(provider).ok_or("An account change is running.")?;
        let native = self.native(provider)?;
        native.preflight()?;
        store::Store::gate(&self.home, &store::ChangeKind::Account)?;
        native.verify()?;
        store::Store::open(&self.home, true)?.change_then(
            store::ChangeKind::Account,
            // Past the gate: a pending recovery refuses the save before the native locks are
            // taken or the native login is read.
            |db| {
                let native_locks = native.lock()?;
                let login = native
                    .read()?
                    .ok_or("Sign in with the official CLI before saving this account.")?;
                let identity = native.identify(&login)?;
                if db
                    .profiles
                    .iter()
                    .any(|p| p.identity == identity && p.pending_activation)
                {
                    return Err("This account has a new saved sign-in awaiting activation. Use it before saving the current login.".into());
                }
                db.reenroll(&identity);
                db.save(identity, login, None)?;
                Ok(native_locks)
            },
            // The native locks cover the save and go before the vault lease, so a publication
            // waiting on the vault never finds them still held.
            |native_locks, _, _| {
                drop(native_locks);
                Ok(())
            },
        )??;
        // The Limits refresh this starts runs outside the provider reservation, which would
        // otherwise keep refusing a use or a sign-out for as long as that read takes.
        drop(read);
        self.notify.changed(provider);
        Ok(())
    }

    fn set_category(&self, id: &str, category: &str) -> Result<(), String> {
        store::Store::open(&self.home, false)?.change(store::ChangeKind::Metadata, |db| {
            db.set_category(id, category)
        })?;
        self.notify.accounts();
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<(), String> {
        login::cancel_expected(id);
        let removed =
            store::Store::open(&self.home, false)?.change(store::ChangeKind::Account, |db| {
                let removed = db.profiles.iter().find(|p| p.id == id).map(|profile| {
                    if !db.ignored_accounts.contains(&profile.identity) {
                        db.ignored_accounts.push(profile.identity.clone());
                    }
                    profile.identity.provider
                });
                db.profiles.retain(|p| p.id != id);
                Ok(removed)
            })?;
        // Its card leaves that provider's Limits now, not at the next poll.
        match removed {
            Some(provider) => self.notify.changed(provider),
            None => self.notify.accounts(),
        }
        Ok(())
    }

    /// The account action a Use or Recover button asks for.
    fn activate(&self, provider: AgentId, id: &str, activation: Activation) -> Result<(), String> {
        match activation {
            Activation::Ordinary => self.use_profile(provider, id, false),
            Activation::AlongsideClients => self.use_profile(provider, id, true),
            Activation::Recover => self.recover(provider),
        }
    }

    /// Publishes profile `id`'s login to the provider's CLI. `alongside_clients` is the person's
    /// choice to switch beside running clients, which then keep the previous account.
    ///
    /// Announced once the change is durable, whether the switch then succeeds or fails: a
    /// rollback may have touched the native login. A sign-in is announced only once published.
    fn use_profile(
        &self,
        provider: AgentId,
        id: &str,
        alongside_clients: bool,
    ) -> Result<(), String> {
        let change = activity::change(provider)?;
        let native = self.native(provider)?;
        native.preflight()?;
        if !alongside_clients {
            self.clients.activation_safe(provider)?;
        }
        let store = store::Store::open(&self.home, false)?;
        let renewals = usage_renew::Renewals::of(&store);
        let native: &dyn Native = native.as_ref();
        let result = store.change_then(
            store::ChangeKind::Account,
            |db| {
                let target = db
                    .profiles
                    .iter()
                    .find(|p| p.id == id && p.identity.provider == provider)
                    .ok_or("Profile does not belong to this provider.")?;
                renewals.activation_ready(target)
            },
            |(), db, persist| transaction::activate(db, native, id, alongside_clients, persist),
        )?;
        drop(change);
        self.notify.changed(provider);
        result
    }

    /// Restores the outgoing login of an interrupted switch, with every client closed.
    /// Announced like a use, whether it then succeeds or fails.
    fn recover(&self, provider: AgentId) -> Result<(), String> {
        let change = activity::change(provider)?;
        let native = self.native(provider)?;
        native.preflight()?;
        self.clients.closed(provider)?;
        let native: &dyn Native = native.as_ref();
        let result = store::Store::open(&self.home, false)?.change_then(
            store::ChangeKind::Recovery,
            |db| {
                let target = db.recovery_target().ok_or("Recovery profile is missing.")?;
                if target.identity.provider != provider {
                    return Err("Recovery belongs to the other provider.".into());
                }
                Ok(())
            },
            |(), db, persist| transaction::recover(db, native, persist),
        )?;
        drop(change);
        self.notify.changed(provider);
        result
    }

    /// Announced once the forgotten logins are durable, whether the logout then succeeds or not:
    /// a failed logout may still have changed the native login.
    fn sign_out(&self, provider: AgentId) -> Result<(), String> {
        let change = activity::change(provider)?;
        let native = self.native(provider)?;
        native.preflight()?;
        self.clients.closed(provider)?;
        store::Store::gate(&self.home, &store::ChangeKind::Account)?;
        let current = native.read()?.ok_or("No native account is signed in.")?;
        let identity = native.identify(&current)?;
        let result = store::Store::open(&self.home, true)?.change_then(
            store::ChangeKind::Account,
            |db| {
                let fingerprint = current.fingerprint();
                if !db.ignored_credentials.contains(&fingerprint) {
                    db.ignored_credentials.push(fingerprint);
                }
                // Logout may revoke shared refresh lineage. Invalidate every saved workspace for
                // this user first.
                for profile in &mut db.profiles {
                    if profile.identity.provider == provider
                        && profile.identity.user_id == identity.user_id
                    {
                        profile.login = None;
                    }
                }
                Ok(())
            },
            |(), _, _| {
                native.logout().and_then(|()| {
                    if native.read()?.is_none() {
                        Ok(())
                    } else {
                        Err("The CLI still reports a login after sign-out.".into())
                    }
                })
            },
        )?;
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
    Ok(store::Store::open_existing(home)?.load()?.codex_claims(key))
}

#[cfg(test)]
pub(crate) use store::tests::saved_codex_fixture;

#[cfg(test)]
mod tests;
