//! A held advisory file lock that is released explicitly when it is dropped.
//!
//! On Unix `File::try_lock` is `flock`, and that lock belongs to the open file description rather
//! than to our descriptor. A child that any thread spawns while the lock is held gets a duplicate
//! of the descriptor and keeps it until it execs, so closing ours can leave the lock held by a
//! process that is still starting. A lease released and then retried at once would report itself
//! busy. Unlocking releases the lock for every duplicate at once.
use std::fs::File;

#[derive(Debug)]
pub(crate) struct FileLease(File);

impl FileLease {
    /// Holds `file` once `lock` has locked it. A failed attempt holds nothing.
    pub(crate) fn acquire<E>(
        file: File,
        lock: impl FnOnce(&File) -> Result<(), E>,
    ) -> Result<Self, E> {
        lock(&file)?;
        Ok(Self(file))
    }

    /// Another descriptor for the same open file, like the one a child spawned while the lease is
    /// held inherits.
    #[cfg(all(test, unix))]
    pub(crate) fn duplicate(&self) -> std::io::Result<File> {
        self.0.try_clone()
    }
}

impl Drop for FileLease {
    fn drop(&mut self) {
        // Closing the file releases the lock too; a failed unlock loses nothing.
        let _ = self.0.unlock();
    }
}

#[cfg(all(test, unix))]
mod tests;
