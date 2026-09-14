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
