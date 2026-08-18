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

    /// Copies one configuration file under `<root>/<agent>/`, keeping the last `KEEP` copies.
    pub fn backup(&self, agent: AgentId, file: &Path) -> Result<Option<PathBuf>, AdapterError> {
        self.snapshot(self.root.join(agent.key()), file)
    }

    /// Copies a whole item (a skill folder or an agent file) under `<root>/<agent>/items/`,
    /// keeping the last `KEEP` copies per name.
    pub fn backup_item(
        &self,
        agent: AgentId,
        item: &Path,
    ) -> Result<Option<PathBuf>, AdapterError> {
        self.snapshot(self.root.join(agent.key()).join("items"), item)
    }

    fn snapshot(&self, dir: PathBuf, source: &Path) -> Result<Option<PathBuf>, AdapterError> {
        if !source.exists() {
            return Ok(None);
        }
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| AdapterError::message("backup target has no file name"))?;
        fs::create_dir_all(&dir).map_err(|error| {
            AdapterError::write(error.to_string(), Some(dir.display().to_string()))
        })?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dest = dir.join(format!("{name}.{stamp}"));
        copy_recursive(source, &dest)?;
        self.prune(&dir, name)?;
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
                    .and_then(|name| name.strip_prefix(&prefix))
                    .is_some_and(|stamp| {
                        !stamp.is_empty() && stamp.bytes().all(|b| b.is_ascii_digit())
                    })
            })
            .collect();
        backups.sort();
        while backups.len() > KEEP {
            let old = backups.remove(0);
            if old.is_dir() {
                let _ = fs::remove_dir_all(old);
            } else {
                let _ = fs::remove_file(old);
            }
        }
        Ok(())
    }
}

fn copy_recursive(from: &Path, to: &Path) -> Result<(), AdapterError> {
    let failed = |error: std::io::Error, path: &Path| {
        AdapterError::write(error.to_string(), Some(path.display().to_string()))
    };
    if from.is_dir() {
        fs::create_dir_all(to).map_err(|error| failed(error, to))?;
        for entry in fs::read_dir(from).map_err(|error| failed(error, from))? {
            let entry = entry.map_err(|error| failed(error, from))?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        fs::copy(from, to).map_err(|error| failed(error, to))?;
    }
    Ok(())
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

    #[test]
    fn backup_item_copies_folders_and_prunes() {
        let root = crate::paths::scratch_dir("on-n-off-backup-item");
        let skill = root.join("tdd");
        fs::create_dir_all(skill.join("ref")).unwrap();
        fs::write(skill.join("SKILL.md"), "v").unwrap();
        fs::write(skill.join("ref/a.md"), "a").unwrap();
        let store = BackupStore::at(root.join("backups"));
        let mut last = None;
        for _ in 0..22 {
            last = store.backup_item(AgentId::Codex, &skill).unwrap();
        }
        let copy = last.unwrap();
        assert_eq!(fs::read_to_string(copy.join("ref/a.md")).unwrap(), "a");
        let dir = root.join("backups/codex/items");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 20);
        let agent = root.join("reviewer.md");
        fs::write(&agent, "x").unwrap();
        let file_copy = store.backup_item(AgentId::Claude, &agent).unwrap().unwrap();
        assert_eq!(fs::read_to_string(file_copy).unwrap(), "x");
        // Pruning `tdd` must not count or remove `tdd.md` copies that share the prefix.
        let same_stem = root.join("tdd.md");
        fs::write(&same_stem, "agent").unwrap();
        for _ in 0..3 {
            store.backup_item(AgentId::Codex, &same_stem).unwrap();
        }
        store.backup_item(AgentId::Codex, &skill).unwrap();
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.iter().filter(|n| n.starts_with("tdd.md.")).count(), 3);
        assert_eq!(names.len(), 23);
        let _ = fs::remove_dir_all(root);
    }
}
