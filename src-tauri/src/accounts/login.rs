//! Isolated official sign-in with operation-scoped cancellation and publication guards.
use super::model::Identity;
use super::transaction::Native;
use super::{home, native, store};
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
    read_usage: impl FnOnce(&store::Login, &Identity) -> Option<crate::dto::ProviderLimitsDto>,
) -> Result<PreparedLogin, String> {
    native.verify()?;
    let login = native
        .read()?
        .ok_or("Official sign-in returned no reusable login.")?;
    let identity = native.identify(&login)?;
    let usage = read_usage(&login, &identity);
    // The official client can rotate its credential while reading usage. Save that latest
    // generation, but never publish a result after a user/workspace change.
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
pub fn add(provider: AgentId, id: String, expected: Option<String>) -> Result<(), String> {
    uuid::Uuid::parse_str(&id).map_err(|_| "Invalid sign-in operation.")?;
    let canceled = LOGIN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .reserve(&id, expected.clone())?;
    let _lease = LoginLease(id.clone());
    let home = home()?;
    let native = native::NativeStore::resolve(provider, &home)?;
    native.preflight()?;
    // Unlock storage before asking the user to sign in, so a denied vault cannot strand a login.
    let ticket = start(&home, provider, expected.as_deref())?;
    let root = home.join(".on-n-off/accounts/logins");
    std::fs::create_dir_all(&root).map_err(|_| "Cannot create isolated sign-in storage.")?;
    let scratch = tempfile::Builder::new()
        .prefix("login-")
        .tempdir_in(&root)
        .map_err(|_| "Cannot prepare isolated sign-in.")?;
    let isolated = native::NativeStore::isolated(provider, scratch.path())?;
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
        &serde_json::to_vec(&provider).map_err(|_| "Cannot record isolated sign-in ownership.")?,
    )?;
    let result: Result<(), String> = (|| {
        let mut command = isolated.command();
        if provider == AgentId::Claude {
            command.args(["auth", "login", "--claudeai"]);
        } else {
            command.arg("login");
        }
        let child = command
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
        let prepared = prepare_login(&isolated, |login, identity| {
            crate::limits::login::read(
                scratch.path(),
                identity,
                crate::limits::credentials::parse_claude_credential(&login.auth),
            )
        })?;
        publish(
            &LOGIN,
            &home,
            &ticket,
            &id,
            &canceled,
            expected.as_deref(),
            prepared,
        )
    })();
    let cleanup = isolated.clean_isolated();
    drop(lease);
    if cleanup.is_err() {
        let _retained = scratch.keep();
        return Err("The isolated login could not be cleaned from protected storage. Its private recovery directory was retained.".into());
    }
    result?;
    // Unlike a use or a sign-out, only a published sign-in changed anything to announce.
    super::changed(provider);
    Ok(())
}

/// What a sign-in is checked against before the official client runs: no pending recovery, and
/// the profile it signs in again still saved for this provider. Gives the ticket its publication
/// is checked against.
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

/// Publishes a finished sign-in into the vault as a profile awaiting activation that owns its
/// renewal: an account change, refused if the sign-in was canceled or its ticket no longer holds.
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
    store::Store::open(home, false)?.change_then(
        store::ChangeKind::SignIn(ticket),
        |db| {
            // Cancellation/removal is checked at publication, not merely when the child exits,
            // and holds off a cancel until the publication is durable.
            let operation = registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if canceled.load(Ordering::Acquire) || !operation.current(id) {
                return Err("Sign-in was canceled.".into());
            }
            db.reenroll(&identity);
            let saved_id = db.save(identity, login, expected)?;
            let profile = db
                .profiles
                .iter_mut()
                .find(|p| p.id == saved_id)
                .ok_or("Saved profile disappeared.")?;
            profile.pending_activation = true;
            profile.usage_renewal_owned = true;
            Ok(operation)
        },
        |operation, _, _| {
            if let Some(usage) = usage {
                // Only a successfully published login may add observations. Quota storage
                // failure must not discard a valid login; its previous history remains untouched.
                let _ = crate::limits::login::remember(home, &usage);
            }
            drop(operation);
            Ok(())
        },
    )?
}

/// Recover only app-created, abandoned login homes after their provider processes have exited.
/// Live leases, unrecognized folders and inaccessible Keychain entries are left intact.
pub(super) fn recover_abandoned(home: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(home.join(".on-n-off/accounts/logins")) else {
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
        if !matches!(provider, AgentId::Claude | AgentId::Codex)
            || super::clients::require_closed(provider).is_err()
        {
            continue;
        }
        let Ok(native) = native::NativeStore::isolated(provider, &path) else {
            continue;
        };
        if native.clean_isolated().is_ok() {
            drop(lease);
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

#[cfg(test)]
mod tests;
