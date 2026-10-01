use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use super::{file_name, read_stored, replaces, SnapshotStore, SNAPSHOT_WRITES};
use crate::dto::{AgentId, LimitsAccountDto};
use crate::usage::cache_io::atomic_write;

pub(super) const ARCHIVE_FILE: &str = "archived.json";

type Archive = BTreeMap<String, BTreeSet<String>>;

impl SnapshotStore {
    pub fn archived(&self, provider: AgentId) -> BTreeSet<String> {
        self.read_archive()
            .remove(provider.key())
            .unwrap_or_default()
    }

    pub fn set_archived(
        &self,
        provider: AgentId,
        ids: &[String],
        archived: bool,
    ) -> Result<bool, String> {
        let _write = SNAPSHOT_WRITES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.edit_archive(provider, |set| {
            ids.iter()
                .filter(|id| !id.trim().is_empty())
                .fold(false, |changed, id| {
                    let edited = if archived {
                        set.insert(id.clone())
                    } else {
                        set.remove(id)
                    };
                    changed | edited
                })
        })
    }

    pub fn unarchive_account(
        &self,
        provider: AgentId,
        account: &LimitsAccountDto,
    ) -> Result<bool, String> {
        let _write = SNAPSHOT_WRITES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let archived = self.archived(provider);
        let mut ids = vec![account.id.clone()];
        if let Some(legacy) = account.legacy_id.as_ref().filter(|legacy| {
            archived.contains(*legacy)
                && read_stored(&self.dir.join(file_name(provider, legacy))).is_some_and(|stored| {
                    stored.provider == provider && replaces(account, &stored.account)
                })
        }) {
            ids.push(legacy.clone());
        }
        self.unarchive_locked(provider, &ids)
    }

    pub(super) fn unarchive_locked(
        &self,
        provider: AgentId,
        ids: &[String],
    ) -> Result<bool, String> {
        self.edit_archive(provider, |set| {
            ids.iter()
                .fold(false, |changed, id| changed | set.remove(id))
        })
    }

    fn read_archive(&self) -> Archive {
        fs::read_to_string(self.dir.join(ARCHIVE_FILE))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn edit_archive(
        &self,
        provider: AgentId,
        edit: impl FnOnce(&mut BTreeSet<String>) -> bool,
    ) -> Result<bool, String> {
        let mut archive = self.read_archive();
        let set = archive.entry(provider.key().to_string()).or_default();
        if !edit(set) {
            return Ok(false);
        }
        archive.retain(|_, ids| !ids.is_empty());
        let path = self.dir.join(ARCHIVE_FILE);
        let json = serde_json::to_string(&archive).map_err(|error| error.to_string())?;
        atomic_write(&path, &json).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
