//! Which accounts the user archived, per provider, in `<home>/.on-n-off/limits/archived.json`
//! beside the snapshots: `{"<provider>": ["<account id>", …]}`, keyed by the ids Forget takes.
//!
//! Only the user archives an account, so nothing here decides to. It is plaintext and needs no
//! vault key: the menu-bar popover and a locked vault honour it, and archiving a history-only card
//! never creates a vault. A missing file archives nothing, and so does a malformed one, which the
//! next write replaces. Writes hold the snapshot lock and replace the file whole.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use super::{SnapshotStore, SNAPSHOT_WRITES};
use crate::dto::AgentId;
use crate::usage::cache_io::atomic_write;

/// The archive's file name: never `<provider>-…`, so the snapshot loader never reads it.
pub(super) const ARCHIVE_FILE: &str = "archived.json";

/// The file's contents: each provider's key to its archived ids.
type Archive = BTreeMap<String, BTreeSet<String>>;

impl SnapshotStore {
    /// The ids of `provider`'s archived accounts.
    pub fn archived(&self, provider: AgentId) -> BTreeSet<String> {
        self.read_archive()
            .remove(provider.key())
            .unwrap_or_default()
    }

    /// Archives or unarchives `ids`, the user's own action; whether the file changed.
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

    /// Takes `ids` out of the archive for a caller that already holds the snapshot lock: Forget,
    /// for the ids whose snapshots it deleted.
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

    /// The archive as its file holds it; empty when there is none or it cannot be read.
    fn read_archive(&self) -> Archive {
        fs::read_to_string(self.dir.join(ARCHIVE_FILE))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    /// Lets `edit` change `provider`'s set, then replaces the file if it did. The caller holds the
    /// snapshot lock.
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
