use super::*;
#[test]
fn authenticates_large_credentials_without_plaintext_on_disk() {
    let data = "refresh-secret".repeat(1000);
    let key = [7; 32];
    let encrypted = seal(&key, data.as_bytes()).unwrap();
    assert!(!encrypted
        .windows(14)
        .any(|bytes| bytes == b"refresh-secret"));
    assert_eq!(unseal(&key, &encrypted).unwrap(), data.as_bytes());
    assert!(unseal(&[8; 32], &encrypted).is_err());
    let mut damaged = encrypted.clone();
    *damaged.last_mut().unwrap() ^= 1;
    assert!(unseal(&key, &damaged).is_err());
    assert_ne!(seal(&key, data.as_bytes()).unwrap(), encrypted);
}
#[test]
fn private_atomic_replacement_preserves_complete_document() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("vault");
    atomic_write(&path, b"first").unwrap();
    atomic_write(&path, b"second").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"second");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn account_and_billing_reads_share_one_vault_unlock_per_storage_root() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let memo = SessionKeys::default();
    let reads = AtomicUsize::new(0);
    for _ in 0..5 {
        assert_eq!(
            memo.get("home-a", false, || {
                reads.fetch_add(1, Ordering::SeqCst);
                Ok([7; 32])
            })
            .unwrap(),
            [7; 32]
        );
    }
    assert_eq!(
        reads.load(Ordering::SeqCst),
        1,
        "each refetch must not request Keychain access again"
    );
    assert_eq!(memo.get("home-b", false, || Ok([8; 32])).unwrap(), [8; 32]);
}

#[test]
fn denied_unlock_is_not_reprompted_by_reads_but_an_explicit_action_can_retry() {
    let memo = SessionKeys::default();
    assert!(memo.get("home", false, || Err("denied".into())).is_err());
    assert_eq!(
        memo.get("home", false, || Ok([7; 32])),
        Err("denied".into())
    );
    assert_eq!(memo.get("home", true, || Ok([8; 32])).unwrap(), [8; 32]);
}

#[test]
fn simultaneous_provider_reads_join_the_same_pending_unlock() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Barrier,
    };
    let memo = Arc::new(SessionKeys::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(3));
    let (entered, started) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let first = {
        let memo = Arc::clone(&memo);
        let calls = Arc::clone(&calls);
        std::thread::spawn(move || {
            memo.get("home", false, || {
                calls.fetch_add(1, Ordering::SeqCst);
                entered.send(()).unwrap();
                wait.recv().unwrap();
                Ok([9; 32])
            })
        })
    };
    started.recv().unwrap();
    let mut readers = Vec::new();
    for _ in 0..2 {
        let memo = Arc::clone(&memo);
        let calls = Arc::clone(&calls);
        let start = Arc::clone(&start);
        readers.push(std::thread::spawn(move || {
            start.wait();
            memo.get("home", false, || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok([1; 32])
            })
        }));
    }
    start.wait();
    // An unrelated store can unlock while this OS prompt is pending: no global mutex held.
    assert_eq!(
        memo.get("other-home", false, || Ok([2; 32])).unwrap(),
        [2; 32]
    );
    release.send(()).unwrap();
    assert_eq!(first.join().unwrap().unwrap(), [9; 32]);
    for reader in readers {
        assert_eq!(reader.join().unwrap().unwrap(), [9; 32]);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// Give cross-domain fixture tests an already-unlocked vault without touching an OS keychain.
pub(crate) fn unlock_fixture(home: &tempfile::TempDir) {
    let root = std::fs::canonicalize(home.path().join(".on-n-off/accounts")).unwrap();
    let scope = crate::sha::sha256_hex(root.to_string_lossy().as_bytes());
    SESSION_KEYS
        .get_or_init(SessionKeys::default)
        .get(&scope, true, || Ok([7; 32]))
        .unwrap();
}
