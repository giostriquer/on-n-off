//! Remembered per-account limit snapshots under `<home>/.on-n-off/limits/`.
//!
//! The CLIs store one login at a time, so switching accounts (`codex login`, `claude`) makes the
//! previous account invisible. Every successful live read is written here (numbers only — never a
//! token) so the screen can keep showing the other accounts as of their last read.

use std::fs;
use std::path::{Path, PathBuf};

use crate::dto::{AgentId, LimitsStatus, ProviderLimitsDto};
use crate::usage::cache_io::atomic_write;

pub struct SnapshotStore {
    dir: PathBuf,
}

impl SnapshotStore {
    pub fn for_home(home: &Path) -> Self {
        Self {
            dir: home.join(".on-n-off").join("limits"),
        }
    }

    #[cfg(test)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Persist a live `ok` snapshot for its account; anything else is refused (`Err`).
    pub fn save(&self, dto: &ProviderLimitsDto) -> Result<(), String> {
        let account = dto
            .account
            .as_ref()
            .ok_or_else(|| "snapshot has no account".to_string())?;
        if dto.status != LimitsStatus::Ok {
            return Err(format!("not saving a {:?} snapshot", dto.status));
        }
        let path = self.dir.join(file_name(dto.provider, &account.id));
        let json = serde_json::to_string(dto).map_err(|error| error.to_string())?;
        atomic_write(&path, &json).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// Every remembered snapshot for `provider`, newest first, marked `live: false`. Unreadable
    /// files are skipped rather than failing the whole read.
    pub fn load(&self, provider: AgentId) -> Vec<ProviderLimitsDto> {
        let prefix = format!("{}-", provider.key());
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut snapshots: Vec<ProviderLimitsDto> = entries
            .flatten()
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with(&prefix) && name.ends_with(".json")
            })
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .filter_map(|raw| serde_json::from_str::<ProviderLimitsDto>(&raw).ok())
            .filter(|dto| dto.provider == provider && dto.account.is_some())
            .map(|mut dto| {
                dto.live = false;
                dto
            })
            .collect();
        snapshots.sort_by(|a, b| b.fetched_at.cmp(&a.fetched_at));
        snapshots
    }

    /// Delete one account's snapshot; unknown accounts are a no-op.
    pub fn forget(&self, provider: AgentId, account_id: &str) -> Result<(), String> {
        let path = self.dir.join(file_name(provider, account_id));
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }
}

/// `<provider>-<account>.json` with the account id reduced to a file-name-safe token; a short
/// hash keeps distinct ids distinct after sanitising.
fn file_name(provider: AgentId, account_id: &str) -> String {
    let safe: String = account_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(48)
        .collect();
    format!("{}-{safe}-{:08x}.json", provider.key(), fnv1a(account_id))
}

fn fnv1a(input: &str) -> u32 {
    input.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{AgentId, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto};
    use crate::paths::scratch_dir;
    use std::fs;

    fn snapshot(provider: AgentId, id: &str, label: &str, fetched_at: &str) -> ProviderLimitsDto {
        ProviderLimitsDto {
            provider,
            status: LimitsStatus::Ok,
            message: None,
            account: Some(LimitsAccountDto {
                id: id.to_string(),
                label: Some(label.to_string()),
            }),
            live: true,
            plan: Some("pro".to_string()),
            windows: vec![super::super::json::window(
                "primary",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                42.0,
                Some("2026-08-24T23:34:33+00:00".to_string()),
            )],
            credits: None,
            fetched_at: fetched_at.to_string(),
        }
    }

    #[test]
    fn saved_snapshots_load_back_per_provider_newest_first_and_marked_not_live() {
        let home = scratch_dir("limits-snap");
        let store = SnapshotStore::for_home(&home);
        store
            .save(&snapshot(
                AgentId::Codex,
                "acct-old",
                "old@x",
                "2026-08-16T10:00:00.000Z",
            ))
            .unwrap();
        store
            .save(&snapshot(
                AgentId::Codex,
                "acct-new",
                "new@x",
                "2026-08-17T10:00:00.000Z",
            ))
            .unwrap();
        store
            .save(&snapshot(
                AgentId::Claude,
                "uuid-1",
                "me@x",
                "2026-08-17T11:00:00.000Z",
            ))
            .unwrap();

        let codex = store.load(AgentId::Codex);
        let ids: Vec<&str> = codex
            .iter()
            .map(|dto| dto.account.as_ref().unwrap().id.as_str())
            .collect();
        assert_eq!(ids, ["acct-new", "acct-old"]);
        assert!(codex
            .iter()
            .all(|dto| !dto.live && dto.status == LimitsStatus::Ok));
        assert_eq!(codex[0].windows[0].used_percent, 42.0);
        assert_eq!(codex[0].plan.as_deref(), Some("pro"));

        let claude = store.load(AgentId::Claude);
        assert_eq!(claude.len(), 1);
        assert_eq!(
            claude[0].account.as_ref().unwrap().label.as_deref(),
            Some("me@x")
        );
    }

    #[test]
    fn saving_the_same_account_again_replaces_its_snapshot() {
        let home = scratch_dir("limits-snap");
        let store = SnapshotStore::for_home(&home);
        store
            .save(&snapshot(
                AgentId::Codex,
                "acct-1",
                "a@x",
                "2026-08-16T10:00:00.000Z",
            ))
            .unwrap();
        let mut newer = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
        newer.windows[0].used_percent = 7.0;
        store.save(&newer).unwrap();
        let loaded = store.load(AgentId::Codex);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].windows[0].used_percent, 7.0);
        assert_eq!(loaded[0].fetched_at, "2026-08-17T10:00:00.000Z");
    }

    #[test]
    fn snapshots_without_an_account_or_not_ok_are_not_saved() {
        let home = scratch_dir("limits-snap");
        let store = SnapshotStore::for_home(&home);
        let mut anonymous = snapshot(AgentId::Codex, "x", "x", "2026-08-17T10:00:00.000Z");
        anonymous.account = None;
        assert!(store.save(&anonymous).is_err());
        let mut failed = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
        failed.status = LimitsStatus::Failed;
        assert!(store.save(&failed).is_err());
        assert!(store.load(AgentId::Codex).is_empty());
    }

    #[test]
    fn forget_removes_one_account_and_load_skips_unreadable_files() {
        let home = scratch_dir("limits-snap");
        let store = SnapshotStore::for_home(&home);
        store
            .save(&snapshot(
                AgentId::Codex,
                "acct-1",
                "a@x",
                "2026-08-16T10:00:00.000Z",
            ))
            .unwrap();
        store
            .save(&snapshot(
                AgentId::Codex,
                "acct-2",
                "b@x",
                "2026-08-17T10:00:00.000Z",
            ))
            .unwrap();
        fs::write(store.dir().join("codex-broken.json"), "{nope").unwrap();
        fs::write(store.dir().join("notes.txt"), "ignore me").unwrap();

        store.forget(AgentId::Codex, "acct-2").unwrap();
        let loaded = store.load(AgentId::Codex);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].account.as_ref().unwrap().id, "acct-1");
        // Forgetting something unknown is not an error.
        store.forget(AgentId::Codex, "acct-2").unwrap();
    }

    #[test]
    fn ids_that_sanitise_alike_stay_distinct_files() {
        let home = scratch_dir("limits-snap");
        let store = SnapshotStore::for_home(&home);
        let long_a = format!("{}A", "x".repeat(60));
        let long_b = format!("{}B", "x".repeat(60));
        for id in ["a/b", "a_b", long_a.as_str(), long_b.as_str()] {
            store
                .save(&snapshot(
                    AgentId::Codex,
                    id,
                    "e@x",
                    "2026-08-17T10:00:00.000Z",
                ))
                .unwrap();
        }
        assert_eq!(store.load(AgentId::Codex).len(), 4);
        store.forget(AgentId::Codex, "a/b").unwrap();
        let ids: Vec<String> = store
            .load(AgentId::Codex)
            .into_iter()
            .map(|dto| dto.account.unwrap().id)
            .collect();
        assert!(!ids.iter().any(|id| id == "a/b"));
        assert!(ids.iter().any(|id| id == "a_b"));
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn a_minimal_older_snapshot_file_still_loads() {
        let home = scratch_dir("limits-snap");
        let store = SnapshotStore::for_home(&home);
        fs::create_dir_all(store.dir()).unwrap();
        fs::write(
            store.dir().join("claude-uuid1-00000000.json"),
            r#"{"provider":"claude","status":"ok","account":{"id":"uuid-1"},"windows":[],"fetchedAt":"2026-08-10T10:00:00.000Z"}"#,
        )
        .unwrap();
        let loaded = store.load(AgentId::Claude);
        assert_eq!(loaded.len(), 1);
        assert!(!loaded[0].live);
        assert_eq!(loaded[0].plan, None);
        assert_eq!(loaded[0].account.as_ref().unwrap().label, None);
    }

    #[test]
    fn account_ids_are_made_safe_for_file_names() {
        let home = scratch_dir("limits-snap");
        let store = SnapshotStore::for_home(&home);
        store
            .save(&snapshot(
                AgentId::Claude,
                "../evil/../id with spaces",
                "e@x",
                "2026-08-17T10:00:00.000Z",
            ))
            .unwrap();
        let files: Vec<String> = fs::read_dir(store.dir())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(files.len(), 1);
        assert!(files[0].starts_with("claude-"), "{files:?}");
        assert!(
            !files[0].contains('/') && !files[0].contains(' ') && !files[0].contains(".."),
            "{files:?}"
        );
        assert_eq!(
            store.load(AgentId::Claude)[0].account.as_ref().unwrap().id,
            "../evil/../id with spaces"
        );
    }
}
