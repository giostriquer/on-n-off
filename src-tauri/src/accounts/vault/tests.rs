use super::*;
const SEALED_BY_CHACHA20POLY1305_0_10: &str = concat!(
    "4f4e4f46462d4143434f554e54532d3100090909090909090909090909090909090909090909090909c55092f813cc10",
    "0e91612a06332e2bc94c9acfee3513f8c39caf6b39b24a363fa24839fbb1e3d43764148ee778b1c6fe66b819281f279b",
    "7acbc48473ad1f19f32dd27b234ee34cd8e69f1b0a6e",
);
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
fn a_vault_sealed_before_the_aead_upgrade_still_opens_and_new_seals_keep_its_layout() {
    let sealed = crate::sha::hex_bytes(SEALED_BY_CHACHA20POLY1305_0_10);
    let plain = br#"{"profiles":[],"fixture":"sealed by chacha20poly1305 0.10.1"}"#;
    assert_eq!(unseal(&[7; 32], &sealed).unwrap(), plain);
    let resealed = seal(&[7; 32], plain).unwrap();
    assert_eq!(resealed.len(), sealed.len());
    assert_eq!(&resealed[..MAGIC.len()], MAGIC);
    assert_ne!(
        &resealed[MAGIC.len()..MAGIC.len() + 24],
        &sealed[MAGIC.len()..MAGIC.len() + 24]
    );
    assert_eq!(unseal(&[7; 32], &resealed).unwrap(), plain);
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
fn account_and_subscription_reads_share_one_vault_unlock_per_storage_root() {
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

pub(crate) fn unlock_fixture(home: &tempfile::TempDir) {
    let root = std::fs::canonicalize(home.path().join(".on-n-off/accounts")).unwrap();
    let scope = crate::sha::sha256_hex(root.to_string_lossy().as_bytes());
    SESSION_KEYS
        .get_or_init(SessionKeys::default)
        .get(&scope, true, || Ok([7; 32]))
        .unwrap();
}

#[test]
#[should_panic(expected = "a test reached the OS credential store")]
fn a_test_that_never_unlocked_its_home_cannot_reach_the_os_credential_store() {
    let home = tempfile::tempdir().unwrap();
    let _ = key(home.path(), true, true);
}
