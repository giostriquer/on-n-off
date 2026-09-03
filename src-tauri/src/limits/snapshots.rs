//! Remembered per-account limit snapshots under `<home>/.on-n-off/limits/`.
//!
//! The CLIs store one login at a time, so switching accounts (`codex login`, `claude`) makes the
//! previous account invisible. Canonical per-window observations are written here (numbers only —
//! never a token) so the screen can keep showing each account's last observations.

use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::dto::{
    AgentId, LimitWindowDto, LimitsAccountDto, LimitsCreditsDto, LimitsStatus, ProviderLimitsDto,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    observed_at: Option<String>,
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

    /// Persist canonical account observations. Dated local or remembered windows remain
    /// trustworthy while refresh is unavailable; a successful credits-only read is dated when it
    /// reaches this storage boundary.
    pub fn save(&self, dto: &ProviderLimitsDto) -> Result<(), String> {
        let account = dto
            .account
            .as_ref()
            .ok_or_else(|| "snapshot has no account".to_string())?;
        if dto.windows.is_empty() && dto.credits.is_none() {
            return Err("snapshot has no observations".to_string());
        }
        let path = self.dir.join(file_name(dto.provider, &account.id));
        let incoming_latest = latest_observed_at(dto)
            .or_else(|| (dto.status == LimitsStatus::Ok && dto.credits.is_some()).then(Utc::now));
        let incoming_latest =
            incoming_latest.ok_or_else(|| "snapshot has no dated observations".to_string())?;
        let existing_latest = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| decode(&raw))
            .and_then(|existing| existing.latest_observed_at());
        if existing_latest.is_some_and(|existing| existing > incoming_latest) {
            return Ok(());
        }
        write_stored(&path, StoredSnapshot::from_dto(dto, incoming_latest))
    }

    /// Every remembered snapshot for `provider`, newest first. Unreadable files are skipped rather
    /// than failing the whole read. Files from obsolete snapshot schemas are ignored.
    pub fn load(&self, provider: AgentId) -> Vec<ProviderLimitsDto> {
        let prefix = format!("{}-", provider.key());
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut snapshots: Vec<(Option<DateTime<Utc>>, ProviderLimitsDto)> = entries
            .flatten()
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with(&prefix) && name.ends_with(".json")
            })
            .filter_map(|entry| {
                let path = entry.path();
                let raw = fs::read_to_string(&path).ok()?;
                let stored = decode(&raw)?;
                Some((stored.latest_observed_at(), stored.into_dto()))
            })
            .filter(|(_, dto)| dto.provider == provider)
            .collect();
        snapshots.sort_by_key(|(observed_at, _)| Reverse(*observed_at));
        snapshots.into_iter().map(|(_, dto)| dto).collect()
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
    fn from_dto(dto: &ProviderLimitsDto, observed_at: DateTime<Utc>) -> Self {
        Self {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            provider: dto.provider,
            account: dto.account.clone().expect("caller checked account"),
            plan: dto.plan.clone(),
            windows: dto.windows.clone(),
            credits: dto.credits.clone(),
            observed_at: Some(observed_at.to_rfc3339_opts(SecondsFormat::Millis, true)),
        }
    }

    fn latest_observed_at(&self) -> Option<DateTime<Utc>> {
        self.observed_at
            .as_deref()
            .and_then(parse_observed_at)
            .or_else(|| latest_window_observed_at(&self.windows))
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

fn decode(raw: &str) -> Option<StoredSnapshot> {
    let stored: StoredSnapshot = serde_json::from_str(raw).ok()?;
    (stored.schema_version == SNAPSHOT_SCHEMA_VERSION).then_some(stored)
}

fn write_stored(path: &Path, stored: StoredSnapshot) -> Result<(), String> {
    let json = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
    atomic_write(path, &json).map_err(|error| format!("{}: {error}", path.display()))
}

fn latest_observed_at(dto: &ProviderLimitsDto) -> Option<DateTime<Utc>> {
    latest_window_observed_at(&dto.windows)
}

fn latest_window_observed_at(windows: &[LimitWindowDto]) -> Option<DateTime<Utc>> {
    windows
        .iter()
        .filter_map(|window| parse_observed_at(&window.observed_at))
        .max()
}

fn parse_observed_at(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|observed_at| observed_at.with_timezone(&Utc))
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
mod tests;
