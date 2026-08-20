//! Remembered per-account limit snapshots under `<home>/.on-n-off/limits/`.
//!
//! The CLIs store one login at a time, so switching accounts (`codex login`, `claude`) makes the
//! previous account invisible. Canonical per-window observations are written here (numbers only —
//! never a token) so the screen can keep showing each account's last observations.

use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::dto::{
    AgentId, LimitWindowDto, LimitWindowKind, LimitsAccountDto, LimitsCreditsDto, LimitsStatus,
    ProviderLimitsDto,
};
use crate::usage::cache_io::atomic_write;

const SNAPSHOT_SCHEMA_VERSION: u8 = 2;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSnapshot {
    schema_version: u8,
    provider: AgentId,
    account: LimitsAccountDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plan: Option<String>,
    windows: Vec<LimitWindowDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credits: Option<LimitsCreditsDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1Snapshot {
    provider: AgentId,
    status: LimitsStatus,
    account: Option<LimitsAccountDto>,
    #[serde(default)]
    plan: Option<String>,
    #[serde(default)]
    windows: Vec<V1Window>,
    #[serde(default)]
    credits: Option<LimitsCreditsDto>,
    fetched_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1Window {
    id: String,
    label: String,
    kind: LimitWindowKind,
    used_percent: f64,
    #[serde(default)]
    resets_at: Option<String>,
}

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

    /// Persist canonical account observations. Endpoint status is deliberately irrelevant: local
    /// or remembered windows can be trustworthy while a provider refresh is unavailable.
    pub fn save(&self, dto: &ProviderLimitsDto) -> Result<(), String> {
        let account = dto
            .account
            .as_ref()
            .ok_or_else(|| "snapshot has no account".to_string())?;
        if dto.windows.is_empty() && dto.credits.is_none() {
            return Err("snapshot has no observations".to_string());
        }
        let path = self.dir.join(file_name(dto.provider, &account.id));
        write_stored(&path, StoredSnapshot::from_dto(dto))
    }

    /// Every remembered snapshot for `provider`, newest first. Unreadable files are skipped rather
    /// than failing the whole read. V1 files are rewritten once at this storage boundary.
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
            .filter_map(|entry| {
                let path = entry.path();
                let raw = fs::read_to_string(&path).ok()?;
                let (stored, migrated) = decode(&raw)?;
                if migrated {
                    let _ = write_stored(&path, stored.clone());
                }
                Some(stored.into_dto())
            })
            .filter(|dto| dto.provider == provider)
            .collect();
        snapshots.sort_by_key(|snapshot| Reverse(latest_observed_at(snapshot)));
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

impl StoredSnapshot {
    fn from_dto(dto: &ProviderLimitsDto) -> Self {
        Self {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            provider: dto.provider,
            account: dto.account.clone().expect("caller checked account"),
            plan: dto.plan.clone(),
            windows: dto.windows.clone(),
            credits: dto.credits.clone(),
        }
    }

    fn into_dto(self) -> ProviderLimitsDto {
        ProviderLimitsDto {
            provider: self.provider,
            status: LimitsStatus::Ok,
            message: None,
            account: Some(self.account),
            current_account: false,
            plan: self.plan,
            windows: self.windows,
            credits: self.credits,
        }
    }
}

fn decode(raw: &str) -> Option<(StoredSnapshot, bool)> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    if value.get("schemaVersion").is_some() {
        let stored: StoredSnapshot = serde_json::from_value(value).ok()?;
        return (stored.schema_version == SNAPSHOT_SCHEMA_VERSION).then_some((stored, false));
    }
    let legacy: V1Snapshot = serde_json::from_value(value).ok()?;
    if legacy.status != LimitsStatus::Ok {
        return None;
    }
    let account = legacy.account?;
    let observed_at = legacy.fetched_at;
    let windows = legacy
        .windows
        .into_iter()
        .map(|window| {
            let window_seconds = infer_window_seconds(window.kind, &window.label);
            LimitWindowDto {
                id: window.id,
                label: window.label,
                kind: window.kind,
                used_percent: window.used_percent,
                resets_at: window.resets_at,
                window_seconds,
                observed_at: observed_at.clone(),
            }
        })
        .collect();
    Some((
        StoredSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            provider: legacy.provider,
            account,
            plan: legacy.plan,
            windows,
            credits: legacy.credits,
        },
        true,
    ))
}

fn infer_window_seconds(kind: LimitWindowKind, label: &str) -> Option<u64> {
    match kind {
        LimitWindowKind::Weekly => Some(7 * 24 * 60 * 60),
        LimitWindowKind::Session => {
            let mut parts = label.split_ascii_whitespace();
            let amount = parts.next()?.parse::<u64>().ok()?;
            match parts.next()? {
                "hour" => Some(amount * 60 * 60),
                "minute" => Some(amount * 60),
                _ => None,
            }
        }
        LimitWindowKind::Model => None,
    }
}

fn write_stored(path: &Path, stored: StoredSnapshot) -> Result<(), String> {
    let json = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
    atomic_write(path, &json).map_err(|error| format!("{}: {error}", path.display()))
}

fn latest_observed_at(dto: &ProviderLimitsDto) -> Option<DateTime<Utc>> {
    dto.windows
        .iter()
        .filter_map(|window| DateTime::parse_from_rfc3339(&window.observed_at).ok())
        .map(|observed_at| observed_at.with_timezone(&Utc))
        .max()
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

    fn snapshot(provider: AgentId, id: &str, label: &str, observed_at: &str) -> ProviderLimitsDto {
        let mut dto = ProviderLimitsDto {
            provider,
            status: LimitsStatus::Ok,
            message: None,
            account: Some(LimitsAccountDto {
                id: id.to_string(),
                label: Some(label.to_string()),
            }),
            current_account: true,
            plan: Some("pro".to_string()),
            windows: vec![super::super::json::window(
                "primary",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                42.0,
                Some("2026-08-24T23:34:33+00:00".to_string()),
            )],
            credits: None,
        };
        for window in &mut dto.windows {
            window.observed_at = observed_at.to_string();
        }
        dto
    }

    #[test]
    fn saved_snapshots_load_back_per_provider_newest_first_and_not_current() {
        let home = scratch_dir("limits-snap");
        let store = SnapshotStore::for_home(&home);
        store
            .save(&snapshot(
                AgentId::Codex,
                "acct-old",
                "old@x",
                "2026-08-17T10:30:00.000+01:00",
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
            .all(|dto| !dto.current_account && dto.status == LimitsStatus::Ok));
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
        assert_eq!(loaded[0].windows[0].observed_at, "2026-08-17T10:00:00.000Z");
    }

    #[test]
    fn snapshots_require_an_account_and_observations_but_not_an_ok_endpoint_status() {
        let home = scratch_dir("limits-snap");
        let store = SnapshotStore::for_home(&home);
        let mut anonymous = snapshot(AgentId::Codex, "x", "x", "2026-08-17T10:00:00.000Z");
        anonymous.account = None;
        assert!(store.save(&anonymous).is_err());
        let mut empty = snapshot(
            AgentId::Codex,
            "acct-empty",
            "empty@x",
            "2026-08-17T10:00:00.000Z",
        );
        empty.windows.clear();
        assert!(store.save(&empty).is_err());
        let mut failed = snapshot(AgentId::Codex, "acct-1", "a@x", "2026-08-17T10:00:00.000Z");
        failed.status = LimitsStatus::Failed;
        assert!(store.save(&failed).is_ok());
        let loaded = store.load(AgentId::Codex);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].account.as_ref().unwrap().id, "acct-1");
        assert_eq!(loaded[0].windows[0].used_percent, 42.0);
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
    fn a_v1_snapshot_migrates_once_to_the_canonical_v2_model() {
        let home = scratch_dir("limits-snap");
        let store = SnapshotStore::for_home(&home);
        fs::create_dir_all(store.dir()).unwrap();
        let path = store.dir().join("claude-uuid1-00000000.json");
        fs::write(
            &path,
            r#"{"provider":"claude","status":"ok","account":{"id":"uuid-1"},"live":true,"plan":"max","windows":[{"id":"weekly_all","label":"Weekly · all models","kind":"weekly","usedPercent":42,"resetsAt":"2026-08-17T10:00:00Z"}],"fetchedAt":"2026-08-10T10:00:00.000Z"}"#,
        )
        .unwrap();
        let loaded = store.load(AgentId::Claude);
        assert_eq!(loaded.len(), 1);
        assert!(!loaded[0].current_account);
        assert_eq!(loaded[0].plan.as_deref(), Some("max"));
        assert_eq!(loaded[0].account.as_ref().unwrap().label, None);
        assert_eq!(loaded[0].windows[0].observed_at, "2026-08-10T10:00:00.000Z");

        let migrated: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(migrated["schemaVersion"], 2);
        assert!(migrated.get("fetchedAt").is_none());
        assert!(migrated.get("live").is_none());
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
