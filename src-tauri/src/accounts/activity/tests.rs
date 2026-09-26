use super::*;
#[test]
fn independent_managers_cannot_refresh_during_activation() {
    let root = tempfile::tempdir().unwrap();
    let first_reader = lease(root.path(), 0, false).unwrap();
    let second_reader = lease(root.path(), 0, false).unwrap();
    assert!(lease(root.path(), 0, true).is_err());
    drop(first_reader);
    drop(second_reader);
    let activation = lease(root.path(), 0, true).unwrap();
    assert!(lease(root.path(), 0, false).is_err());
    assert!(lease(root.path(), 0, true).is_err());
    assert!(lease(root.path(), 1, false).is_ok());
    drop(activation);
    assert!(lease(root.path(), 0, false).is_ok());
}
#[cfg(unix)]
#[test]
fn released_leases_are_free_while_a_spawned_child_still_shares_them() {
    // A child that another thread spawns meanwhile inherits the descriptor until it execs.
    let root = tempfile::tempdir().unwrap();
    let activation = lease(root.path(), 0, true).unwrap();
    let inherited = activation.duplicate().unwrap();
    drop(activation);
    assert!(lease(root.path(), 0, false).is_ok());
    drop(inherited);
    let reader = lease(root.path(), 0, false).unwrap();
    let inherited = reader.duplicate().unwrap();
    drop(reader);
    assert!(lease(root.path(), 0, true).is_ok());
    drop(inherited);
}

/// Whether no read or change of `provider` is running in this process. Only tests that hold no
/// reservation of their own concurrently, such as the serialized account-operation tests, can rely
/// on it.
pub(crate) fn idle(provider: AgentId) -> bool {
    let Some(i) = index(provider) else {
        return true;
    };
    let reading = ACTIVITY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)[i];
    reading == 0 && !SWITCHING[i].load(Ordering::Acquire)
}
