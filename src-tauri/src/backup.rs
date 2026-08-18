use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::dto::{AdapterError, AgentId};

const KEEP: usize = 20;

pub struct BackupStore {
    root: PathBuf,
}

impl BackupStore {
    pub fn new() -> Result<Self, AdapterError> {
        Ok(Self {
            root: crate::paths::backup_root()?,
        })
    }

    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn backup(&self, agent: AgentId, file: &Path) -> Result<Option<PathBuf>, AdapterError> {
        if !file.exists() {
            return Ok(None);
        }
        let filename = file
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| AdapterError::message("backup target has no file name"))?;
        let dir = self.root.join(agent.key());
        fs::create_dir_all(&dir).map_err(|error| {
            AdapterError::write(error.to_string(), Some(dir.display().to_string()))
        })?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dest = dir.join(format!("{filename}.{stamp}"));
        fs::copy(file, &dest).map_err(|error| {
            AdapterError::write(error.to_string(), Some(dest.display().to_string()))
        })?;
        self.prune(&dir, filename)?;
        Ok(Some(dest))
    }

    pub fn restore(&self, backup: &Path, dest: &Path) -> Result<(), AdapterError> {
        fs::copy(backup, dest).map_err(|error| {
            AdapterError::write(error.to_string(), Some(dest.display().to_string()))
        })?;
        Ok(())
    }

    fn prune(&self, dir: &Path, filename: &str) -> Result<(), AdapterError> {
        let prefix = format!("{filename}.");
        let mut backups: Vec<_> = fs::read_dir(dir)
            .map_err(|error| {
                AdapterError::write(error.to_string(), Some(dir.display().to_string()))
            })?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
            })
            .collect();
        backups.sort();
        while backups.len() > KEEP {
            let old = backups.remove(0);
            let _ = fs::remove_file(old);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_copies_file_and_prunes_to_last_twenty() {
        let root = crate::paths::scratch_dir("on-n-off-backup");
        let file = root.join("settings.json");
        fs::write(&file, "{\"a\":1}").unwrap();
        let store = BackupStore::at(root.join("backups"));
        for i in 0..22 {
            fs::write(&file, format!("{{\"a\":{i}}}")).unwrap();
            store.backup(AgentId::Claude, &file).unwrap();
        }
        let dir = root.join("backups/claude");
        let count = fs::read_dir(&dir).unwrap().count();
        assert_eq!(count, 20);
        let _ = fs::remove_dir_all(root);
    }
}
