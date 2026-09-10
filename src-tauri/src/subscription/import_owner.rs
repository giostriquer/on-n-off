//! Owns native imports until reaped and cleaned. Shared leases protect crash recovery
//! while either the app or its helper still uses a private snapshot directory.
use std::{
    fs::File,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Condvar, Mutex,
    },
    time::Duration,
};
#[derive(Default)]
pub(super) struct ImportOwner {
    stopping: AtomicBool,
    active: Mutex<usize>,
    finished: Condvar,
}
pub(super) static OWNER: ImportOwner = ImportOwner {
    stopping: AtomicBool::new(false),
    active: Mutex::new(0),
    finished: Condvar::new(),
};
impl ImportOwner {
    pub(super) fn run(
        &self,
        mut command: Command,
        scratch: Scratch,
    ) -> std::io::Result<crate::process::CommandOutcome> {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.stopping.load(Ordering::Acquire) {
            return Err(std::io::Error::other("Browser import is shutting down."));
        }
        *active += 1;
        drop(active);
        let permit = Permit(self);
        let result = (|| {
            let child = command.spawn()?;
            crate::process::wait_with_cancellation(child, Duration::from_secs(60), || {
                self.stopping.load(Ordering::Acquire)
            })
        })();
        // The process has been reaped before its cookie snapshots are removed, and
        // shutdown cannot return until that removal has finished.
        let cleanup = scratch.close();
        drop(permit);
        cleanup?;
        result
    }
    pub(super) fn shutdown(&self) {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.stopping.store(true, Ordering::Release);
        while *active != 0 {
            active = self
                .finished
                .wait(active)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
}
struct Permit<'a>(&'a ImportOwner);
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut active = self
            .0
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active -= 1;
        self.0.finished.notify_all();
    }
}
pub(super) fn root() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let path = std::env::temp_dir().join("on-n-off-billing-imports");
    match std::fs::DirBuilder::new().mode(0o700).create(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    if !path.symlink_metadata()?.file_type().is_dir() {
        return Err(std::io::Error::other("Invalid browser import directory."));
    }
    Ok(path)
}
pub(super) struct Scratch {
    directory: tempfile::TempDir,
    _lease: File,
}
impl Scratch {
    pub(super) fn path(&self) -> &Path {
        self.directory.path()
    }
    fn close(self) -> std::io::Result<()> {
        self.directory.close()
    }
}
fn root_lock(root: &Path) -> std::io::Result<File> {
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("recovery.lock"))?;
    file.lock()?;
    Ok(file)
}
pub(super) fn private_scratch(root: &Path) -> std::io::Result<Scratch> {
    // Serialize creation with recovery until the new directory holds its lease.
    let _root_lock = root_lock(root)?;
    let directory = tempfile::Builder::new()
        .prefix("import-")
        .tempdir_in(root)?;
    let lease = File::create(directory.path().join("lease"))?;
    lease.lock_shared()?;
    Ok(Scratch {
        directory,
        _lease: lease,
    })
}
pub(super) fn recover(root: &Path) {
    let Ok(_root_lock) = root_lock(root) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("import-")
            || !entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        let Ok(file) = File::open(entry.path().join("lease")) else {
            continue;
        };
        // Both parent and helper hold a shared lease. Never remove a live import,
        // including one from another app instance or a helper whose parent crashed.
        if file.try_lock().is_ok() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}
#[cfg(test)]
mod tests;
