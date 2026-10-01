pub(crate) mod model;
pub(crate) mod vault;

mod store;
#[cfg(test)]
pub(crate) use store::lease_timeout_override::set as override_lease_timeout;
pub(crate) mod usage;
mod usage_renew;

mod transaction;

mod homes;

pub(crate) mod claude;
pub(crate) mod claude_store;
pub(crate) mod codex;
pub(crate) mod codex_store;
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
use transaction::{Native, NativeGuard};
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
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    archived: bool,
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

const PROVIDERS: [AgentId; 2] = [AgentId::Claude, AgentId::Codex];

trait Adapter: Sync {
    fn native(&self, home: &Path) -> Result<Box<dyn NativeAccount>, String>;
    fn isolated(&self, dir: &Path) -> Result<Box<dyn IsolatedSignIn>, String>;
    fn login<'a>(&self, login: &'a store::Login) -> Box<dyn model::LoginView + 'a>;
    fn client(&self) -> &'static clients::Client;
    fn token_url(&self) -> Option<&'static str>;
    fn renew_private(
        &self,
        login: &store::Login,
        now_ms: i64,
        token_url: &str,
    ) -> Result<store::Login, String>;
    fn read_usage(
        &self,
        identity: &model::Identity,
        login: &store::Login,
        now_ms: i64,
        urls: &crate::limits::SavedReadUrls<'_>,
    ) -> Result<crate::dto::ProviderLimitsDto, crate::limits::SavedReadError>;
    fn homes(&self) -> Option<HomeAt>;
    fn renews_privately(&self) -> bool {
        self.homes().is_none()
    }
}

type HomeAt = fn(&Path) -> Box<dyn Home>;
type MakeHome<'a> = dyn Fn(&Path) -> Box<dyn Home> + 'a;

fn adapter(provider: AgentId) -> Result<&'static dyn Adapter, String> {
    match provider {
        AgentId::Claude => Ok(&claude::Claude),
        AgentId::Codex => Ok(&codex::Codex),
        _ => Err("Saved subscription profiles are not supported for this provider.".into()),
    }
}

fn view(provider: AgentId, login: &store::Login) -> Result<Box<dyn model::LoginView + '_>, String> {
    adapter(provider).map(|adapter| adapter.login(login))
}

trait NativeAccount: Native {
    fn preflight(&self) -> Result<(), String>;
    fn logout(&self) -> Result<(), String>;
    fn subscription(&self) -> Result<Option<model::Identity>, String> {
        self.read()?.map(|login| self.identify(&login)).transpose()
    }
}

trait IsolatedSignIn: Native {
    fn sign_in(&self) -> std::process::Command;
    fn first_usage(
        &self,
        dir: &Path,
        identity: &model::Identity,
    ) -> Option<crate::dto::ProviderLimitsDto>;
    fn clean(&self) -> Result<(), String>;
}

trait Home: Send + Sync {
    fn lock(&self) -> Result<Box<dyn NativeGuard>, String>;
    fn read(&self) -> Result<Option<store::Login>, String>;
    fn identify(&self, login: &store::Login) -> Result<model::Identity, String>;
    fn put(
        &self,
        login: &store::Login,
        locks: &dyn NativeGuard,
    ) -> Result<Option<store::Login>, String>;
    fn clear(&self, locks: &dyn NativeGuard) -> Result<(), String>;
    fn read_usage(
        &self,
        identity: &model::Identity,
    ) -> Result<crate::dto::ProviderLimitsDto, crate::limits::SavedReadError>;
    fn delete(&self, locks: Box<dyn NativeGuard>) -> Result<(), String>;
}

trait Clients {
    fn activation_safe(&self, provider: AgentId) -> Result<(), String>;
    fn closed(&self, provider: AgentId) -> Result<(), String>;
}

trait Notify {
    fn changed(&self, provider: AgentId);
    fn accounts(&self);
}

trait NativeStores {
    fn native(&self, provider: AgentId, home: &Path) -> Result<Box<dyn NativeAccount>, String>;
    fn isolated(&self, provider: AgentId, dir: &Path) -> Result<Box<dyn IsolatedSignIn>, String>;
    fn homes(&self, provider: AgentId) -> Option<Box<MakeHome<'_>>>;
}

struct AdapterStores;
impl NativeStores for AdapterStores {
    fn native(&self, provider: AgentId, home: &Path) -> Result<Box<dyn NativeAccount>, String> {
        adapter(provider)?.native(home)
    }
    fn isolated(&self, provider: AgentId, dir: &Path) -> Result<Box<dyn IsolatedSignIn>, String> {
        adapter(provider)?.isolated(dir)
    }
    fn homes(&self, provider: AgentId) -> Option<Box<MakeHome<'_>>> {
        let at = adapter(provider).ok()?.homes()?;
        Some(Box::new(at))
    }
}

struct Accounts {
    home: PathBuf,
    stores: Box<dyn NativeStores>,
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
        crate::limits_refresh::account_changed(provider);
        crate::read_revision::announce(crate::read_revision::Source::Accounts);
    }
    fn accounts(&self) {
        crate::read_revision::announce(crate::read_revision::Source::Accounts);
    }
}

impl Accounts {
    fn live() -> Result<Self, String> {
        Ok(Self {
            home: home()?,
            stores: Box::new(AdapterStores),
            clients: Box::new(RunningClients),
            notify: Box::new(Announce),
        })
    }
    fn native(&self, provider: AgentId) -> Result<Box<dyn NativeAccount>, String> {
        self.stores.native(provider, &self.home)
    }
    fn isolated(&self, provider: AgentId, dir: &Path) -> Result<Box<dyn IsolatedSignIn>, String> {
        self.stores.isolated(provider, dir)
    }
    fn account_homes(
        &self,
        provider: AgentId,
    ) -> Option<impl Fn(&str) -> Result<Box<dyn Home>, String> + '_> {
        let at = self.stores.homes(provider)?;
        Some(move |id: &str| Ok(at(&homes::dir(&self.home, id)?)))
    }
}

pub fn list(provider: AgentId) -> Result<AccountsDto, String> {
    Accounts::live()?.list(provider)
}
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
pub fn add(provider: AgentId, id: String, expected: Option<String>) -> Result<(), String> {
    Accounts::live()?.add(provider, id, expected)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    Ordinary,
    AlongsideClients,
    Recover,
}
impl Activation {
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
        self.recover_abandoned();
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
        let archived = crate::limits::archived(home, provider);
        Ok(AccountsDto {
            profiles: db
                .profiles
                .into_iter()
                .filter(|p| p.identity.provider == provider)
                .map(|p| ProfileDto {
                    needs_login: p.needs_login(),
                    archived: archived.contains(&p.identity.observation_key()),
                    observation_id: p.identity.observation_key(),
                    active: current.as_ref() == Some(&p.identity),
                    id: p.id,
                    identity: p.identity,
                    label: p.label,
                    email: p.email,
                    category: p.category,
                    saved_at: p.saved_at,
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
        let (identity, email) = store::Store::open(&self.home, true)?.change_then(
            store::ChangeKind::Account,
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
                let id = db.save(identity.clone(), login, None)?;
                let email = db
                    .profiles
                    .iter()
                    .find(|p| p.id == id)
                    .and_then(|p| p.email.clone());
                Ok((native_locks, identity, email))
            },
            |(native_locks, identity, email), _, _| {
                drop(native_locks);
                Ok((identity, email))
            },
        )??;
        let _ = crate::limits::unarchive_profile(&self.home, &identity, email);
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
        match removed {
            Some(provider) => self.notify.changed(provider),
            None => self.notify.accounts(),
        }
        Ok(())
    }

    fn activate(&self, provider: AgentId, id: &str, activation: Activation) -> Result<(), String> {
        match activation {
            Activation::Ordinary => self.use_profile(provider, id, false),
            Activation::AlongsideClients => self.use_profile(provider, id, true),
            Activation::Recover => self.recover(provider),
        }
    }

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
        let account_homes = self.account_homes(provider);
        let result = store.change_then(
            store::ChangeKind::Account,
            |db| {
                let target = db
                    .profiles
                    .iter()
                    .find(|p| p.id == id && p.identity.provider == provider)
                    .ok_or("Profile does not belong to this provider.")?;
                renewals.activation_ready(target)?;
                match &account_homes {
                    Some(account_homes) => homes::check_out(db, id, account_homes),
                    None => Ok(None),
                }
            },
            |checked_out, db, persist| {
                if let Some(checked_out) = checked_out {
                    checked_out.empty().map_err(|error| {
                        format!("{error} Its login stays in its home; nothing was replaced.")
                    })?;
                }
                transaction::activate(db, native, id, alongside_clients, persist)
            },
        )?;
        drop(change);
        self.notify.changed(provider);
        result
    }

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
                let fingerprint = view(provider, &current)?.fingerprint();
                if !db.ignored_credentials.contains(&fingerprint) {
                    db.ignored_credentials.push(fingerprint);
                }
                for profile in &mut db.profiles {
                    if profile.identity.provider == provider
                        && profile.identity.user_id == identity.user_id
                    {
                        profile.login = None;
                        profile.home = None;
                        profile.signed_out = true;
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

mod login;
pub use login::cancel;

pub(crate) mod discovery;

pub(crate) fn saved_codex_claims(
    home: &Path,
    key: &str,
) -> Result<Option<serde_json::Value>, String> {
    if !model::Identity::is_profile_key(key) || !store::Store::vault_exists(home) {
        return Ok(None);
    }
    let db = store::Store::open_existing(home)?.load()?;
    Ok(db
        .observed(AgentId::Codex, key)
        .and_then(|profile| profile.login.as_ref())
        .and_then(|login| codex::CodexLogin::of(login).auth_claims().ok().flatten()))
}

#[cfg(test)]
pub(crate) use store::tests::saved_codex_fixture;

#[cfg(test)]
mod tests;
