//! Excludes native account mutation from in-process provider reads without holding a mutex
//! across network I/O. Shared/exclusive file leases also exclude other app processes; the
//! separate vault lease serializes protected database publication.
use super::PROVIDERS;
use crate::{dto::AgentId, file_lease::FileLease};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
static ACTIVITY: Mutex<[usize; PROVIDERS.len()]> = Mutex::new([0; PROVIDERS.len()]);
static SWITCHING: [AtomicBool; PROVIDERS.len()] =
    [const { AtomicBool::new(false) }; PROVIDERS.len()];
/// The provider's slot; `None` for a provider without saved profiles, whose reads nothing excludes.
fn index(provider: AgentId) -> Option<usize> {
    PROVIDERS.iter().position(|p| *p == provider)
}
pub struct Read(Option<usize>, Option<FileLease>);
pub fn read(provider: AgentId) -> Option<Read> {
    let i = index(provider);
    let mut counts = ACTIVITY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(i) = i {
        if SWITCHING[i].load(Ordering::Acquire) {
            return None;
        }
        counts[i] += 1;
    }
    drop(counts);
    let mut guard = Read(i, None);
    if let Some(i) = i {
        guard.1 = runtime_lease(i, false).ok()?;
    }
    Some(guard)
}
impl Drop for Read {
    fn drop(&mut self) {
        if let Some(i) = self.0 {
            ACTIVITY
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)[i] -= 1;
        }
    }
}
pub struct Change(usize, Option<FileLease>);
pub fn change(provider: AgentId) -> Result<Change, String> {
    let i = index(provider).ok_or("Account activation is unsupported.")?;
    let counts = ACTIVITY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if counts[i] > 0 || SWITCHING[i].swap(true, Ordering::AcqRel) {
        return Err("A provider read or account change is running. Retry when it finishes.".into());
    }
    drop(counts);
    let mut guard = Change(i, None);
    guard.1 = runtime_lease(i, true)?;
    Ok(guard)
}
impl Drop for Change {
    fn drop(&mut self) {
        SWITCHING[self.0].store(false, Ordering::Release);
    }
}

/// Separate from the protected vault lease: ordinary reads must not unlock the vault.
fn lease(home: &std::path::Path, provider: usize, exclusive: bool) -> Result<FileLease, String> {
    let root = home.join(".on-n-off/accounts");
    std::fs::create_dir_all(&root).map_err(|_| "Cannot coordinate native account access.")?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(format!("activity-{provider}.lock")))
        .map_err(|_| "Cannot coordinate native account access.")?;
    FileLease::acquire(file, |file| {
        if exclusive {
            file.try_lock()
        } else {
            file.try_lock_shared()
        }
    })
    .map_err(|_| {
        "Another app instance is reading or changing this account. Retry when it finishes.".into()
    })
}
#[cfg(test)]
pub(crate) mod tests;

#[cfg(not(test))]
fn runtime_lease(provider: usize, exclusive: bool) -> Result<Option<FileLease>, String> {
    lease(
        &crate::paths::user_home().map_err(|_| "Cannot resolve account home.")?,
        provider,
        exclusive,
    )
    .map(Some)
}
#[cfg(test)]
fn runtime_lease(_: usize, _: bool) -> Result<Option<FileLease>, String> {
    Ok(None)
}
