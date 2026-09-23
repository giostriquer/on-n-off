//! The duplicate-descriptor tests are Unix only: a duplicate shares the lock only where locks
//! belong to the open file description, and Windows opens handles that a child cannot inherit.
use super::FileLease;
use std::{fs::File, path::Path};

fn open(dir: &Path) -> File {
    File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("lease.lock"))
        .unwrap()
}

#[cfg(unix)]
#[test]
fn a_dropped_lease_is_free_while_a_duplicate_descriptor_survives() {
    let dir = tempfile::tempdir().unwrap();
    let lease = FileLease::acquire(open(dir.path()), File::try_lock).unwrap();
    let inherited = lease.duplicate().unwrap();
    assert!(
        open(dir.path()).try_lock().is_err(),
        "the lease must exclude another open of the same file"
    );
    drop(lease);
    assert!(
        FileLease::acquire(open(dir.path()), File::try_lock).is_ok(),
        "a dropped lease must not wait for every duplicate descriptor to close"
    );
    drop(inherited);
}

#[cfg(unix)]
#[test]
fn a_shared_lease_is_released_the_same_way() {
    let dir = tempfile::tempdir().unwrap();
    let lease = FileLease::acquire(open(dir.path()), File::try_lock_shared).unwrap();
    let inherited = lease.duplicate().unwrap();
    assert!(open(dir.path()).try_lock().is_err());
    drop(lease);
    assert!(FileLease::acquire(open(dir.path()), File::try_lock).is_ok());
    drop(inherited);
}

#[test]
fn a_busy_lease_reports_the_lock_error() {
    let dir = tempfile::tempdir().unwrap();
    let holder = FileLease::acquire(open(dir.path()), File::try_lock).unwrap();
    assert!(matches!(
        FileLease::acquire(open(dir.path()), File::try_lock),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    drop(holder);
    assert!(FileLease::acquire(open(dir.path()), File::try_lock).is_ok());
}
