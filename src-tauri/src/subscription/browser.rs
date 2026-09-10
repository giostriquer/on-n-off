//! Throttled browser-session imports. Cookies stay in a short-lived native helper; only
//! account-checked billing metadata crosses stdout. Automatic imports require prior success and suppress Keychain interaction.
use super::{store, SubscriptionDate};
use std::{
    collections::HashSet,
    path::Path,
    sync::{Mutex, MutexGuard},
};
#[derive(Default)]
struct State {
    generation: u64,
    active: Option<String>,
    unavailable: HashSet<String>,
}
static STATE: Mutex<Option<State>> = Mutex::new(None);
fn state() -> MutexGuard<'static, Option<State>> {
    STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
#[cfg(any(target_os = "macos", test))]
fn parse_reply(reply: &str, account: &str) -> Result<SubscriptionDate, String> {
    let value: serde_json::Value =
        serde_json::from_str(reply).map_err(|_| "Invalid browser billing response.")?;
    if let Some(error) = value.get("error").and_then(serde_json::Value::as_str) {
        return Err(if error == "accountMismatch" {
            "Your browser is signed in to a different ChatGPT account or workspace. Switch to the account shown on this Codex card, then retry."
        } else {
            "No usable ChatGPT browser session was found. Sign in in your browser and allow cookie access, then retry."
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
) -> Result<(), String> {
    if state.generation != generation || state.active.as_deref() != Some(account) {
        return Err("Browser import was canceled.".into());
    }
    state.active = None;
    let result = super::validate_account(home, account)
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
    let generation = {
        let mut lock = state();
        let state = lock.get_or_insert_with(State::default);
        if state.active.is_some() {
            return Err("A browser import is already running.".into());
        }
        state.generation += 1;
        state.active = Some(account.clone());
        state.generation
    };
    // A failed manual retry must not immediately trigger an automatic retry via its event.
    let _ = store::claim_auto_refresh(&home, &account, chrono::Utc::now());
    let result = fetch(&account, false);
    let result = {
        let mut lock = state();
        publish(
            &home,
            lock.as_mut().expect("initialized state"),
            generation,
            &account,
            result,
        )
    };
    crate::read_revision::announce(crate::read_revision::Source::SubscriptionCodex);
    result
}
fn reserve_auto(
    home: &Path,
    state: &mut State,
    account: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<u64> {
    if state.active.is_some()
        || super::validate_account(home, account).is_err()
        || !store::claim_auto_refresh(home, account, now)
    {
        return None;
    }
    state.generation += 1;
    state.active = Some(account.into());
    Some(state.generation)
}
// Reserve before spawning; event-driven rereads see the reservation and do not recurse.
pub fn refresh_if_due(home: &Path, account: &str) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let generation = {
        let mut lock = state();
        let state = lock.get_or_insert_with(State::default);
        let Some(generation) = reserve_auto(home, state, account, chrono::Utc::now()) else {
            return;
        };
        generation
    };
    let home = home.to_path_buf();
    let account = account.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let result = fetch(&account, true);
        {
            let mut lock = state();
            let _ = publish(
                &home,
                lock.as_mut().expect("initialized state"),
                generation,
                &account,
                result,
            );
        }
        crate::read_revision::announce(crate::read_revision::Source::SubscriptionCodex);
    });
}
pub fn disconnect(_app: &tauri::AppHandle, account: Option<&str>) -> Result<(), String> {
    let mut lock = state();
    let state = lock.get_or_insert_with(State::default);
    if account.is_none() || state.active.as_deref() == account {
        state.generation += 1;
        state.active = None;
    }
    Ok(())
}
pub fn forget(home: &Path, account: &str) -> Result<(), String> {
    let mut lock = state();
    let state = lock.get_or_insert_with(State::default);
    if state.active.as_deref() == Some(account) {
        state.generation += 1;
        state.active = None;
    }
    state.unavailable.remove(account);
    store::forget(home, account)
}
pub fn read_metadata(
    home: &Path,
    account: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> super::SubscriptionReading {
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
    super::SubscriptionReading {
        metadata: super::choose(billing, local),
        connected: false,
        unavailable,
        browser_supported: cfg!(target_os = "macos"),
    }
}
#[cfg(target_os = "macos")]
fn fetch(account: &str, non_interactive: bool) -> Result<SubscriptionDate, String> {
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
        .arg(account);
    if non_interactive {
        command.arg("--non-interactive");
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match super::import_owner::OWNER.run(command, scratch) {
        Ok(crate::process::CommandOutcome::Exited {success:true, stdout, ..}) if stdout.len() <= 4096 => parse_reply(&stdout, account),
        _ => Err("Could not read a matching browser session. Sign in to ChatGPT in Safari, Chrome, Edge, Brave, or Firefox, allow cookie access if prompted, then retry.".into()),
    }
}
#[cfg(not(target_os = "macos"))]
fn fetch(_account: &str, _non_interactive: bool) -> Result<SubscriptionDate, String> {
    Err("Browser billing import is currently available on macOS only.".into())
}
#[cfg(test)]
mod tests;
