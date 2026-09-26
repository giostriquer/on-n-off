//! Opt-in discovery of verified native logins; never activates an account.
//! Consent is checked before credential access and again under the publication lease.
use super::{
    model::Identity,
    store::{ChangeKind, Database, Guard, Login, Store, Ticket},
    transaction::Native,
};
use crate::dto::AgentId;
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Mutex, time::Duration};

const INTERVAL: Duration = Duration::from_secs(30);
#[derive(Default, Serialize, Deserialize)]
struct Preferences {
    #[serde(default)]
    enabled: bool,
}
static NOTICES: Mutex<[Option<String>; 2]> = Mutex::new([None, None]);
struct AccountDiscovery;

pub fn enabled(home: &Path) -> Result<bool, String> {
    match std::fs::read(home.join(".on-n-off/accounts/remembering.json")) {
        Ok(bytes) => serde_json::from_slice::<Preferences>(&bytes)
            .map(|p| p.enabled)
            .map_err(|_| {
                "The account remembering preference is unreadable. Enable it again to resume."
                    .into()
            }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("Cannot read the account remembering preference.".into()),
    }
}
pub fn set_enabled(remember: bool) -> Result<(), String> {
    let home = super::home()?;
    set_enabled_in(&home, remember, &|create| Store::open(&home, create))?;
    for provider in [AgentId::Claude, AgentId::Codex] {
        update_notice(provider, None);
    }
    crate::read_revision::announce(crate::read_revision::Source::Accounts);
    Ok(())
}
fn set_enabled_in(
    home: &Path,
    remember: bool,
    open: &dyn Fn(bool) -> Result<Store, String>,
) -> Result<(), String> {
    if !remember {
        // Consent must remain revocable without opening protected credentials.
        let (root, _lease) = Store::lease(home)?;
        return super::vault::atomic_write(&root.join("remembering.json"), b"{\"enabled\":false}");
    }
    let store = open(true)?;
    let consent = store.root.join("remembering.json");
    // Consent is written only once the bumped epoch is durable, under the same lease.
    store.change_then(
        ChangeKind::Remembering,
        |_| Ok(()),
        |(), _, _| super::vault::atomic_write(&consent, b"{\"enabled\":true}"),
    )?
}

fn index(provider: AgentId) -> usize {
    if provider == AgentId::Claude {
        0
    } else {
        1
    }
}
pub fn notice(provider: AgentId) -> Option<String> {
    NOTICES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)[index(provider)]
    .clone()
}
fn update_notice(provider: AgentId, message: Option<String>) {
    let changed = {
        let mut notices = NOTICES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let slot = &mut notices[index(provider)];
        if *slot == message {
            false
        } else {
            *slot = message;
            true
        }
    };
    if changed {
        crate::read_revision::announce(crate::read_revision::Source::Accounts);
    }
}
fn eligible(db: &Database, identity: &Identity, login: &Login) -> Result<bool, String> {
    let fingerprint = super::view(identity.provider, login)?.fingerprint();
    Ok(!db.ignored_accounts.contains(identity)
        && !db.ignored_credentials.contains(&fingerprint)
        && !db.profiles.iter().any(|p| {
            p.identity == *identity && (p.pending_activation || p.login.as_ref() == Some(login))
        }))
}
fn candidate(native: &dyn Native, db: &Database) -> Result<Option<Login>, String> {
    let Some(login) = native.read()? else {
        return Ok(None);
    };
    let identity = native.identify(&login)?;
    if !eligible(db, &identity, &login)? {
        return Ok(None);
    }
    native.verify_observed()?;
    if native.read()?.as_ref() != Some(&login) {
        return Err(
            "The CLI login changed during verification. Automatic remembering will check again."
                .into(),
        );
    }
    Ok(Some(login))
}
/// Saves `login` as a profile once consent, the ticket, the exclusions and the exact native
/// credential all still hold under the vault lease, holding the native locks through the save.
fn publish(
    store: Store,
    ticket: &Ticket,
    native: &dyn Native,
    login: Login,
    remember: bool,
) -> Result<bool, String> {
    if !remember {
        return Ok(false);
    }
    store.publish_then(
        ticket,
        |db| {
            let identity = native.identify(&login)?;
            if !eligible(db, &identity, &login)? {
                return Ok(None);
            }
            let locks = native.lock()?;
            if native.read()?.as_ref() != Some(&login) {
                return Err("The CLI login changed before it could be saved. Automatic remembering will check again.".into());
            }
            locks.ensure()?;
            db.save(identity, login, None)?;
            Ok(Some(locks))
        },
        // The native locks cover the save and go before the vault lease.
        |locks, _, _| Ok(locks.is_some()),
    )?
}
fn poll_home(home: &Path, provider: AgentId) -> Result<bool, String> {
    if !enabled(home)? {
        return Ok(false);
    }
    let _read = super::activity::read(provider)
        .ok_or("An account change is running. Automatic remembering will retry.")?;
    let native = super::native::NativeStore::resolve(provider, home)?;
    native.preflight()?;
    let db = Store::open_existing(home)?.load()?;
    // Nothing is remembered while an interrupted switch awaits recovery.
    let Ok(ticket) = db.ticket(Guard::SignIn) else {
        return Ok(false);
    };
    let Some(login) = candidate(&native, &db)? else {
        return Ok(false);
    };
    // Network verification holds no vault lease. Recheck consent, epoch, recovery, exclusions
    // and the exact native credential under the leases before publishing its protected copy.
    let store = Store::open_existing(home)?;
    let remember = enabled(home)?;
    publish(store, &ticket, &native, login, remember)
}
pub fn setup(app: &mut tauri::App) {
    crate::monitor::spawn::<AccountDiscovery, _, _>(app, run);
}
pub fn wake(app: &tauri::AppHandle) {
    crate::monitor::wake::<AccountDiscovery>(app);
}
async fn run(_app: tauri::AppHandle, mut wake: tauri::async_runtime::Receiver<()>) {
    loop {
        for provider in [AgentId::Claude, AgentId::Codex] {
            let result = tauri::async_runtime::spawn_blocking(move || {
                let home = super::home()?;
                poll_home(&home, provider)
            })
            .await
            .unwrap_or_else(|_| Err("Automatic account remembering could not complete.".into()));
            match result {
                Ok(changed) => {
                    update_notice(provider, None);
                    if changed {
                        crate::read_revision::announce(crate::read_revision::Source::Accounts);
                    }
                }
                Err(message) => update_notice(provider, Some(message)),
            }
        }
        crate::monitor::wait_for_wake_or_deadline(&mut wake, INTERVAL).await;
    }
}
#[cfg(test)]
mod tests;
