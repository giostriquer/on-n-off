//! Best-effort durable writes for derived Usage caches.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn temp_sibling(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{}.tmp-on-n-off", std::process::id()));
    PathBuf::from(name)
}

pub fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    atomic_write_with(path, contents, || Ok(()))
}

fn atomic_write_with(
    path: &Path,
    contents: &str,
    before_replace: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temporary = temp_sibling(path);
    let write_result = (|| {
        let mut file = File::create(&temporary)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        before_replace()?;
        fs::rename(&temporary, path)
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    write_result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::scratch_dir;

    #[test]
    fn failed_replacement_preserves_existing_cache() {
        let root = scratch_dir("usage-atomic-write-failure");
        let path = root.join("cache.json");
        fs::write(&path, r#"{"version":1}"#).unwrap();

        let result = atomic_write_with(&path, r#"{"version":2}"#, || {
            Err(io::Error::other("injected replacement failure"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"version":1}"#);
        assert!(!temp_sibling(&path).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn readers_only_observe_complete_documents_during_replacement() {
        let root = scratch_dir("usage-atomic-write-readers");
        let path = root.join("cache.json");
        fs::write(&path, r#"{"generation":0}"#).unwrap();
        let reader_path = path.clone();
        let reader = std::thread::spawn(move || {
            for _ in 0..500 {
                let raw = fs::read_to_string(&reader_path).unwrap();
                assert!(serde_json::from_str::<serde_json::Value>(&raw).is_ok());
            }
        });
        for generation in 1..=100 {
            atomic_write(&path, &format!(r#"{{"generation":{generation}}}"#)).unwrap();
        }
        reader.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }
}
