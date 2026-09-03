//! Best-effort durable writes for derived Usage caches.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn temp_sibling(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(
        ".{}.{}.tmp-on-n-off",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
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
mod tests;
