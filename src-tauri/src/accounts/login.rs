use super::model::Identity;
use super::store;
use super::transaction::Native;
use crate::{dto::AgentId, file_lease::FileLease};
use std::{
    collections::VecDeque,
    path::Path,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
struct LoginOperation {
    id: String,
    expected: Option<String>,
    cancel: Arc<AtomicBool>,
}
#[derive(Default)]
struct Registry {
    active: Option<LoginOperation>,
    canceled: VecDeque<String>,
}
impl Registry {
    fn reserve(&mut self, id: &str, expected: Option<String>) -> Result<Arc<AtomicBool>, String> {
        if self.canceled.iter().any(|c| c == id) {
            return Err("Sign-in was canceled before it started.".into());
        }
        if self.active.is_some() {
            return Err("A sign-in is already running.".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.active = Some(LoginOperation {
            id: id.into(),
            expected,
            cancel: cancel.clone(),
        });
        Ok(cancel)
    }
    fn cancel(&mut self, id: &str) {
        if let Some(op) = self.active.as_ref().filter(|op| op.id == id) {
            op.cancel.store(true, Ordering::Release);
        } else {
            self.canceled.push_back(id.into());
            if self.canceled.len() > 128 {
                self.canceled.pop_front();
            }
        }
    }
    fn current(&self, id: &str) -> bool {
        self.active
            .as_ref()
            .is_some_and(|op| op.id == id && !op.cancel.load(Ordering::Acquire))
    }
}
static LOGIN: Mutex<Registry> = Mutex::new(Registry {
    active: None,
    canceled: VecDeque::new(),
});
pub fn cancel(id: &str) {
    LOGIN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .cancel(id);
}
pub(super) fn cancel_expected(id: &str) {
    if let Some(op) = LOGIN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .active
        .as_ref()
    {
        if op.expected.as_deref() == Some(id) {
            op.cancel.store(true, Ordering::Release);
        }
    }
}
struct LoginLease(String);

struct PreparedLogin {
    identity: Identity,
    login: store::Login,
    usage: Option<crate::dto::ProviderLimitsDto>,
}

fn prepare_login(
    native: &dyn Native,
    read_usage: impl FnOnce(&Identity) -> Option<crate::dto::ProviderLimitsDto>,
) -> Result<PreparedLogin, String> {
    native.verify()?;
    let login = native
        .read()?
        .ok_or("Official sign-in returned no reusable login.")?;
    let identity = native.identify(&login)?;
    let usage = read_usage(&identity);
    let latest = native
        .read()?
        .ok_or("Sign-in disappeared while reading usage.")?;
    if native.identify(&latest)? != identity {
        return Err("The signed-in account changed while reading usage. Retry sign-in.".into());
    }
    let usage = usage
        .filter(|dto| {
            dto.provider == identity.provider
                && dto.status == crate::dto::LimitsStatus::Ok
                && dto.account.as_ref().is_some_and(|account| {
                    account.id == identity.observation_key()
                        && match (
                            account.label.as_deref(),
                            super::view(identity.provider, &latest)
                                .ok()
                                .and_then(|latest| latest.email()),
                        ) {
                            (Some(observed), Some(email)) => {
                                observed.trim().eq_ignore_ascii_case(&email)
                            }
                            _ => false,
                        }
                })
        })
        .map(|mut dto| {
            dto.current_account = false;
            dto
        });
    Ok(PreparedLogin {
        identity,
        login: latest,
        usage,
    })
}
impl Drop for LoginLease {
    fn drop(&mut self) {
        let mut op = LOGIN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if op.active.as_ref().is_some_and(|op| op.id == self.0) {
            op.active = None;
        }
    }
}
impl super::Accounts {
    pub(super) fn add(
        &self,
        provider: AgentId,
        id: String,
        expected: Option<String>,
    ) -> Result<(), String> {
        uuid::Uuid::parse_str(&id).map_err(|_| "Invalid sign-in operation.")?;
        let canceled = LOGIN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reserve(&id, expected.clone())?;
        let _lease = LoginLease(id.clone());
        let home = &self.home;
        let native = self.native(provider)?;
        native.preflight()?;
        let ticket = start(home, provider, expected.as_deref())?;
        let root = home.join(".on-n-off/accounts/logins");
        std::fs::create_dir_all(&root).map_err(|_| "Cannot create isolated sign-in storage.")?;
        let scratch = tempfile::Builder::new()
            .prefix("login-")
            .tempdir_in(&root)
            .map_err(|_| "Cannot prepare isolated sign-in.")?;
        let isolated = self.isolated(provider, scratch.path())?;
        let lease_file = std::fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(scratch.path().join("lease"))
            .map_err(|_| "Cannot own isolated sign-in storage.")?;
        let lease = FileLease::acquire(lease_file, std::fs::File::try_lock)
            .map_err(|_| "Cannot own isolated sign-in storage.")?;
        super::vault::atomic_write(
            &scratch.path().join("provider.json"),
            &serde_json::to_vec(&provider)
                .map_err(|_| "Cannot record isolated sign-in ownership.")?,
        )?;
        let result: Result<(), String> = (|| {
            let child = isolated
                .sign_in()
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|_| {
                    "Could not start official sign-in. Check the provider CLI installation."
                })?;
            match crate::process::wait_with_cancellation(child,Duration::from_secs(300),||canceled.load(Ordering::Acquire)){
   Ok(crate::process::CommandOutcome::Exited{success:true,..})=>{},
   _=>return Err("Sign-in was canceled, timed out, or did not complete. The current native login was not changed.".into())
  }
            if canceled.load(Ordering::Acquire) {
                return Err("Sign-in was canceled.".into());
            }
            let prepared = prepare_login(isolated.as_ref(), |identity| {
                isolated.first_usage(scratch.path(), identity)
            })?;
            publish(
                &LOGIN,
                home,
                &ticket,
                &id,
                &canceled,
                expected.as_deref(),
                prepared,
            )
        })();
        let cleanup = isolated.clean();
        drop(lease);
        if cleanup.is_err() {
            let _retained = scratch.keep();
            return Err("The isolated login could not be cleaned from protected storage. Its private recovery directory was retained.".into());
        }
        result?;
        self.notify.changed(provider);
        Ok(())
    }

    pub(super) fn recover_abandoned(&self) {
        let Ok(entries) = std::fs::read_dir(self.home.join(".on-n-off/accounts/logins")) else {
            return;
        };
        for entry in entries.flatten().take(128) {
            let path = entry.path();
            if !entry.file_name().to_string_lossy().starts_with("login-")
                || !entry
                    .file_type()
                    .is_ok_and(|t| t.is_dir() && !t.is_symlink())
            {
                continue;
            }
            let Ok(file) = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path.join("lease"))
            else {
                continue;
            };
            let Ok(lease) = FileLease::acquire(file, std::fs::File::try_lock) else {
                continue;
            };
            let Some(provider) = std::fs::read(path.join("provider.json"))
                .ok()
                .filter(|v| v.len() < 100)
                .and_then(|v| serde_json::from_slice::<AgentId>(&v).ok())
            else {
                continue;
            };
            let Ok(isolated) = self.isolated(provider, &path) else {
                continue;
            };
            if self.clients.closed(provider).is_err() {
                continue;
            }
            if isolated.clean().is_ok() {
                drop(lease);
                let _ = std::fs::remove_dir_all(path);
            }
        }
    }
}

fn start(home: &Path, provider: AgentId, expected: Option<&str>) -> Result<store::Ticket, String> {
    let store = store::Store::open(home, true)?;
    let db = store.load()?;
    let ticket = db.ticket(store::Guard::SignIn)?;
    if let Some(id) = expected {
        if !db
            .profiles
            .iter()
            .any(|p| p.id == id && p.identity.provider == provider)
        {
            return Err("Profile no longer exists.".into());
        }
    }
    Ok(ticket)
}

fn publish(
    registry: &Mutex<Registry>,
    home: &Path,
    ticket: &store::Ticket,
    id: &str,
    canceled: &AtomicBool,
    expected: Option<&str>,
    prepared: PreparedLogin,
) -> Result<(), String> {
    let PreparedLogin {
        identity,
        login,
        usage,
    } = prepared;
    let signed_in = identity.clone();
    store::Store::open(home, false)?.change_then(
        store::ChangeKind::SignIn(ticket),
        |db| {
            let operation = registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if canceled.load(Ordering::Acquire) || !operation.current(id) {
                return Err("Sign-in was canceled.".into());
            }
            db.reenroll(&identity);
            let renews_privately = super::adapter(identity.provider)?.renews_privately();
            let saved_id = db.save(identity, login, expected)?;
            let profile = db
                .profiles
                .iter_mut()
                .find(|p| p.id == saved_id)
                .ok_or("Saved profile disappeared.")?;
            profile.home = None;
            profile.pending_activation = true;
            profile.usage_renewal_owned = renews_privately;
            Ok((operation, profile.email.clone()))
        },
        |(operation, email), _, _| {
            if let Some(usage) = usage {
                let _ = crate::limits::remember(home, usage);
            }
            let _ = crate::limits::unarchive_profile(home, &signed_in, email);
            drop(operation);
            Ok(())
        },
    )?
}

#[cfg(test)]
mod tests;
