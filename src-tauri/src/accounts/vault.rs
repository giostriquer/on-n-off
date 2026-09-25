//! Versioned authenticated encryption. Only the 32-byte key enters the OS credential store;
//! large OAuth envelopes remain encrypted on disk. There is no plaintext fallback.
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    XChaCha20Poly1305, XNonce,
};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};
const MAGIC: &[u8] = b"ONOFF-ACCOUNTS-1\0";
pub fn seal(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let encrypted = cipher
        .encrypt(&nonce, data)
        .map_err(|_| "Could not encrypt saved profiles.")?;
    Ok([MAGIC, nonce.as_slice(), &encrypted].concat())
}
pub fn unseal(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, String> {
    let data = data
        .strip_prefix(MAGIC)
        .filter(|v| v.len() >= 40)
        .ok_or("Invalid or unsupported account vault.")?;
    XChaCha20Poly1305::new(key.into())
        .decrypt(XNonce::from_slice(&data[..24]), &data[24..])
        .map_err(|_| {
            "The account vault could not be unlocked or is damaged. It has not been replaced."
                .into()
        })
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid storage path.")?;
    fs::create_dir_all(parent).map_err(|_| "Cannot create account storage.")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "Cannot prepare private account storage.")?;
    file.write_all(bytes)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| "Cannot write account storage.")?;
    file.persist(path)
        .map_err(|_| "Cannot replace account storage.")?;
    #[cfg(unix)]
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|_| "Cannot sync account storage.")?;
    Ok(())
}
/// Only the vault encryption key is retained for this app session, never native OAuth payloads.
/// Concurrent account and subscription reads join one unlock; a denied read waits for an explicit retry.
type KeyResult = Result<[u8; 32], String>;
#[derive(Default)]
struct SessionKeys(Mutex<HashMap<String, Arc<OnceLock<KeyResult>>>>);
impl SessionKeys {
    fn get(&self, scope: &str, retry: bool, read: impl FnOnce() -> KeyResult) -> KeyResult {
        let slot = {
            let mut entries = self
                .0
                .lock()
                .map_err(|_| "Account unlock state is unavailable.")?;
            let slot = entries.entry(scope.to_owned()).or_default();
            if retry && slot.get().is_some_and(Result::is_err) {
                *slot = Arc::default();
            }
            Arc::clone(slot)
        };
        // No map mutex or account-storage lease is held during the OS authorization prompt.
        slot.get_or_init(read).clone()
    }
}
static SESSION_KEYS: OnceLock<SessionKeys> = OnceLock::new();

pub fn key(root: &Path, create: bool, retry: bool) -> KeyResult {
    let canonical = fs::canonicalize(root).map_err(|_| "Cannot resolve profile storage.")?;
    let scope = crate::sha::sha256_hex(canonical.to_string_lossy().as_bytes());
    SESSION_KEYS
        .get_or_init(SessionKeys::default)
        .get(&scope, retry, || read_key(&scope, create))
}
/// A test that reaches the OS credential store has forgotten `tests::unlock_fixture` for its home;
/// failing it here keeps the suite from reading or writing whoever runs it's real vault keys.
#[cfg(test)]
fn read_key(scope: &str, _create: bool) -> KeyResult {
    panic!("a test reached the OS credential store for vault scope {scope}; unlock its home with vault::tests::unlock_fixture")
}
#[cfg(not(test))]
fn read_key(scope: &str, create: bool) -> KeyResult {
    if !cfg!(any(target_os = "macos", windows)) {
        return Err("Protected profiles require macOS or Windows.".into());
    }
    let entry = keyring::Entry::new("on-n-off.accounts.v1", scope)
        .map_err(|_| "Cannot open protected account storage.")?;
    match entry.get_secret() {
        Ok(bytes) => bytes
            .try_into()
            .map_err(|_| "Invalid account vault key.".into()),
        Err(keyring::Error::NoEntry) if create => {
            let key = XChaCha20Poly1305::generate_key(&mut OsRng);
            entry.set_secret(&key).map_err(|_| {
                "Could not protect the account vault key. Allow access to the OS credential store."
            })?;
            Ok(key.into())
        }
        _ => Err("Could not unlock saved accounts.".into()),
    }
}

#[cfg(test)]
pub(crate) mod tests;
