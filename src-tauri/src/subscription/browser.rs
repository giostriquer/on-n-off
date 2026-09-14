//! Throttled browser-session imports. Cookies stay in a short-lived native helper; only
//! account-checked billing metadata crosses stdout. Automatic imports require a saved account or prior success and suppress browser Keychain interaction.
use super::{store, SubscriptionDate};
use std::{
    collections::HashSet,
    path::Path,
    sync::{Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};
#[derive(Default)]
struct State {
    generation: u64,
    active: Option<String>,
    unavailable: HashSet<String>,
    waiting_manual: usize,
    cancellation: u64,
}
#[derive(Default)]
struct Imports {
    state: Mutex<Option<State>>,
    available: Condvar,
}
static IMPORTS: Imports = Imports {
    state: Mutex::new(None),
    available: Condvar::new(),
};
fn state() -> MutexGuard<'static, Option<State>> {
    IMPORTS
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
impl Imports {
    fn reserve_manual(&self, account: &str, timeout: Duration) -> Result<u64, String> {
        let deadline = Instant::now() + timeout;
        let mut lock = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = lock.get_or_insert_with(State::default);
        let cancellation = state.cancellation;
        state.waiting_manual += 1;
        self.available.notify_all();
        loop {
            let state = lock.as_mut().expect("initialized state");
            if state.cancellation != cancellation {
                state.waiting_manual -= 1;
                return Err(
                    "Billing check was canceled because the accounts changed. Retry billing."
                        .into(),
                );
            }
            if state.active.is_none() {
                state.waiting_manual -= 1;
                state.generation += 1;
                state.active = Some(account.into());
                return Ok(state.generation);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                state.waiting_manual -= 1;
                return Err(
                    "The previous billing check has not finished. Retry billing in a moment."
                        .into(),
                );
            }
            // Waiting releases STATE, so imports can finish and account changes can cancel us.
            (lock, _) = self
                .available
                .wait_timeout(lock, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
}
#[cfg(any(target_os = "macos", test))]
fn parse_reply(reply: &str, account: &str) -> Result<SubscriptionDate, String> {
    let value: serde_json::Value =
        serde_json::from_str(reply).map_err(|_| "Invalid browser billing response.")?;
    if let Some(error) = value.get("error").and_then(serde_json::Value::as_str) {
        return Err(if error == "accountMismatch" {
            "The browser sessions that could be read did not verify this account or workspace. The correct browser profile may be inaccessible."
        } else {
            "Could not verify billing for this account. Check browser cookie access and the selected ChatGPT workspace, then retry."
        }.into());
    }
    if value.get("accountId").and_then(serde_json::Value::as_str) != Some(account) {
        return Err("The browser session does not match this Codex account.".into());
    }
    super::parse_billing(&value, chrono::Utc::now())
        .ok_or_else(|| "Billing date unavailable.".into())
}
fn publish(
    home: &Path,
    state: &mut State,
    generation: u64,
    account: &str,
    result: Result<SubscriptionDate, String>,
    validation: Result<(), String>,
) -> Result<(), String> {
    if state.generation != generation || state.active.as_deref() != Some(account) {
        return Err("Browser import was canceled.".into());
    }
    state.active = None;
    let result = validation
        .and(result)
        .and_then(|metadata| store::save(home, account, &metadata));
    if result.is_ok() {
        state.unavailable.remove(account);
    } else {
        state.unavailable.insert(account.into());
    }
    result
}
pub fn connect(_app: &tauri::AppHandle, account: String) -> Result<(), String> {
    let home = crate::paths::user_home().map_err(|_| "Could not find your home directory.")?;
    super::validate_account(&home, &account)?;
    let generation = IMPORTS.reserve_manual(&account, Duration::from_secs(90))?;
    // A failed manual retry must not immediately trigger an automatic retry via its event.
    let _ = store::record_attempt(&home, &account, chrono::Utc::now(), true);
    let result = fetch(&account, false);
    let result = finish(&home, generation, &account, result);
    crate::read_revision::announce(crate::read_revision::Source::SubscriptionCodex);
    result
}
fn finish(
    home: &Path,
    generation: u64,
    account: &str,
    result: Result<SubscriptionDate, String>,
) -> Result<(), String> {
    // Resolve the vault before taking STATE; retain its operation lease through publication.
    let result = super::with_billing_context(home, account, |context| {
        let validation = super::validate_context(context);
        let mut lock = state();
        publish(
            home,
            lock.as_mut().expect("initialized state"),
            generation,
            account,
            result,
            validation,
        )
    });
    IMPORTS.available.notify_all();
    result
}
fn reserve_auto(
    home: &Path,
    state: &mut State,
    account: &str,
    now: chrono::DateTime<chrono::Utc>,
    context: Option<&super::BillingContext>,
) -> Option<u64> {
    if state.active.is_some()
        || state.waiting_manual > 0
        || context.is_none()
        || (!context.is_some_and(|c| c.saved) && store::load(home, account).is_none())
        || !store::claim_auto_refresh(home, account, now)
    {
        return None;
    }
    state.generation += 1;
    state.active = Some(account.into());
    Some(state.generation)
}
// Reserve before spawning; event-driven rereads see the reservation and do not recurse.
pub(super) fn refresh_if_due(home: &Path, account: &str, context: Option<&super::BillingContext>) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let generation = {
        let mut lock = state();
        let state = lock.get_or_insert_with(State::default);
        let Some(generation) = reserve_auto(home, state, account, chrono::Utc::now(), context)
        else {
            return;
        };
        generation
    };
    let home = home.to_path_buf();
    let account = account.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let result = fetch(&account, true);
        let _ = finish(&home, generation, &account, result);
        crate::read_revision::announce(crate::read_revision::Source::SubscriptionCodex);
    });
}
pub fn disconnect(_app: &tauri::AppHandle, account: Option<&str>) -> Result<(), String> {
    let mut lock = state();
    let state = lock.get_or_insert_with(State::default);
    state.cancellation += 1;
    if account.is_none() || state.active.as_deref() == account {
        state.generation += 1;
        state.active = None;
    }
    drop(lock);
    IMPORTS.available.notify_all();
    Ok(())
}
pub fn forget(home: &Path, account: &str) -> Result<(), String> {
    let mut lock = state();
    let state = lock.get_or_insert_with(State::default);
    state.cancellation += 1;
    if state.active.as_deref() == Some(account) {
        state.generation += 1;
        state.active = None;
    }
    state.unavailable.remove(account);
    let result = store::forget(home, account);
    drop(lock);
    IMPORTS.available.notify_all();
    result
}
pub(super) fn read_metadata(
    home: &Path,
    account: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(super::SubscriptionReading, Option<super::BillingContext>), String> {
    let context = super::billing_context(home, account)?;
    let can_connect = context.is_some();
    let unavailable = state()
        .as_ref()
        .is_some_and(|state| state.unavailable.contains(account));
    let billing = store::load(home, account).map(|mut metadata| {
        metadata.stale = unavailable
            || metadata
                .checked_at
                .as_deref()
                .and_then(|at| at.parse::<chrono::DateTime<chrono::Utc>>().ok())
                .is_none_or(|at| at > now || (now - at).num_seconds() >= super::REFRESH_SECONDS);
        metadata
    });
    let local = store::identity(home)
        .filter(|(id, _)| id == account)
        .and_then(|(_, claims)| super::parse_claims(&claims, account, now));
    Ok((
        super::SubscriptionReading {
            metadata: super::choose(billing, local),
            connected: false,
            unavailable,
            browser_supported: cfg!(target_os = "macos"),
            can_connect,
        },
        context,
    ))
}
#[cfg(target_os = "macos")]
fn fetch(account: &str, non_interactive: bool) -> Result<SubscriptionDate, String> {
    let home = crate::paths::user_home().map_err(|_| "Cannot resolve current billing identity.")?;
    let context =
        super::billing_context(&home, account)?.ok_or("Cannot verify billing account identity.")?;
    let workspace = &context.workspace;
    let user = &context.user;

    use std::process::{Command, Stdio};
    let exe = std::env::current_exe().map_err(|_| "Cannot locate browser billing helper.")?;
    let folder = exe
        .parent()
        .ok_or("Cannot locate browser billing helper.")?;
    let folder = if cfg!(test) && folder.file_name().is_some_and(|name| name == "deps") {
        folder
            .parent()
            .ok_or("Cannot locate browser billing helper.")?
    } else {
        folder
    };
    let helper = if folder.file_name().is_some_and(|name| name == "MacOS") {
        folder.join("../Helpers/on-n-off-billing")
    } else {
        folder.join("on-n-off-billing")
    };
    let root =
        super::import_owner::root().map_err(|_| "Cannot create private browser import storage.")?;
    super::import_owner::recover(&root);
    let scratch = super::import_owner::private_scratch(&root)
        .map_err(|_| "Cannot create private browser import storage.")?;
    let mut command = Command::new(helper);
    command
        .env("TMPDIR", scratch.path())
        .env("ON_N_OFF_IMPORT_LEASE", scratch.path().join("lease"))
        .arg(workspace)
        .arg(user);
    if non_interactive {
        command.arg("--non-interactive");
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match super::import_owner::OWNER.run(command, scratch) {
        Ok(crate::process::CommandOutcome::Exited {success:true, stdout, ..}) if stdout.len() <= 4096 => { super::validate_account(&home,account)?; parse_reply(&stdout, workspace) },
        _ => Err("Could not read a matching browser session. Sign in to ChatGPT in Safari, Chrome, Edge, Brave, or Firefox, allow cookie access if prompted, then retry.".into()),
    }
}
#[cfg(not(target_os = "macos"))]
fn fetch(_account: &str, _non_interactive: bool) -> Result<SubscriptionDate, String> {
    Err("Browser billing import is currently available on macOS only.".into())
}

/// A native login change invalidates any in-flight browser observation.
pub(crate) fn invalidate_identity() {
    let mut lock = state();
    let state = lock.get_or_insert_with(State::default);
    state.generation += 1;
    state.cancellation += 1;
    state.active = None;
    drop(lock);
    IMPORTS.available.notify_all();
    crate::read_revision::announce(crate::read_revision::Source::SubscriptionCodex);
}

#[cfg(test)]
mod tests;
