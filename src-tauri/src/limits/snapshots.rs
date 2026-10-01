use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use super::reading::{keep_remembered, newest, parse_observed_at};
use crate::dto::{AgentId, LimitsAccountDto, LimitsStatus, ProviderLimitsDto, Reading};
use crate::usage::cache_io::atomic_write;

mod archive;

const SNAPSHOT_SCHEMA_VERSION: u8 = 2;
static SNAPSHOT_WRITES: Mutex<()> = Mutex::new(());

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSnapshot {
    schema_version: u8,
    provider: AgentId,
    account: LimitsAccountDto,
    #[serde(flatten)]
    reading: Reading,
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

    pub fn remember(&self, mut card: ProviderLimitsDto) -> Remembered {
        let _write = SNAPSHOT_WRITES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(path) = self.path_of(&card) else {
            return Remembered {
                card,
                saved: Err("snapshot has no account".to_string()),
            };
        };
        let existing = read_stored(&path);
        if let Some(existing) = &existing {
            keep_remembered(&mut card, existing.clone().into_dto(Utc::now()).reading);
        }
        let saved = write_over(&path, &card, existing.as_ref());
        Remembered { card, saved }
    }

    pub fn save(&self, dto: &ProviderLimitsDto) -> Result<(), String> {
        let _write = SNAPSHOT_WRITES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = self
            .path_of(dto)
            .ok_or_else(|| "snapshot has no account".to_string())?;
        write_over(&path, dto, read_stored(&path).as_ref())
    }

    fn path_of(&self, dto: &ProviderLimitsDto) -> Option<PathBuf> {
        let account = dto.account.as_ref()?;
        Some(self.dir.join(file_name(dto.provider, &account.id)))
    }

    pub fn save_changed(&self, before: &[ProviderLimitsDto], after: &[ProviderLimitsDto]) {
        for (changed, previous) in after.iter().zip(before) {
            if changed != previous {
                let _ = self.save(changed);
            }
        }
    }

    pub fn load(&self, provider: AgentId) -> Vec<ProviderLimitsDto> {
        without_superseded(
            self.stored(provider)
                .into_iter()
                .filter(|dto| dto.reading.has_observations())
                .collect(),
        )
    }

    fn stored(&self, provider: AgentId) -> Vec<ProviderLimitsDto> {
        let now = Utc::now();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut snapshots: Vec<(Option<DateTime<Utc>>, ProviderLimitsDto)> = entries
            .flatten()
            .filter(|entry| is_snapshot_file(provider, &entry.file_name().to_string_lossy()))
            .filter_map(|entry| {
                let path = entry.path();
                let raw = fs::read_to_string(&path).ok()?;
                let stored = decode(&raw)?;
                Some((stored.latest_observed_at(), stored.into_dto(now)))
            })
            .filter(|(_, dto)| dto.provider == provider)
            .collect();
        snapshots.sort_by_key(|(observed_at, _)| Reverse(*observed_at));
        snapshots.into_iter().map(|(_, dto)| dto).collect()
    }

    pub fn forget_matching_email(
        &self,
        provider: AgentId,
        account_id: &str,
        email: &str,
    ) -> Result<(), String> {
        let _write = SNAPSHOT_WRITES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changed = "Account history changed. Refresh accounts before removing it.";
        if email.trim().is_empty() || account_id.starts_with("profile:") {
            return Err(changed.into());
        }
        let path = self.dir.join(file_name(provider, account_id));
        let forgotten = [account_id.to_string()];
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return self.unarchive_locked(provider, &forgotten).map(drop);
            }
            Err(_) => return Err(changed.into()),
        };
        let stored = decode(&raw).ok_or(changed)?;
        if stored.provider != provider
            || stored.account.id != account_id
            || !stored
                .account
                .label
                .as_deref()
                .is_some_and(|label| label.trim().eq_ignore_ascii_case(email.trim()))
        {
            return Err(changed.into());
        }
        self.remove_file(provider, account_id)?;
        self.unarchive_locked(provider, &forgotten).map(drop)
    }

    pub fn forget(&self, provider: AgentId, account_id: &str) -> Result<(), String> {
        let _write = SNAPSHOT_WRITES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let all = self.stored(provider);
        let mut forgotten = Vec::new();
        if let Some(target) = all
            .iter()
            .find(|dto| dto.account.as_ref().is_some_and(|a| a.id == account_id))
        {
            for legacy in all.iter().filter(|dto| supersedes(target, dto)) {
                let id = &legacy.account.as_ref().expect("matched account").id;
                self.remove_file(provider, id)?;
                forgotten.push(id.clone());
            }
        }
        self.remove_file(provider, account_id)?;
        forgotten.push(account_id.to_string());
        self.unarchive_locked(provider, &forgotten).map(drop)
    }

    fn remove_file(&self, provider: AgentId, account_id: &str) -> Result<(), String> {
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
            reading: Reading {
                reset_offer: None,
                ..dto.reading.clone()
            },
            observed_at: Some(observed_at.to_rfc3339_opts(SecondsFormat::Millis, true)),
        }
    }

    fn latest_observed_at(&self) -> Option<DateTime<Utc>> {
        self.observed_at
            .as_deref()
            .and_then(parse_observed_at)
            .or_else(|| newest(&self.reading.windows))
    }

    fn into_dto(self, now: DateTime<Utc>) -> ProviderLimitsDto {
        let mut reading = self.reading.known_at(now);
        match self.provider {
            AgentId::Codex => super::codex::drop_hidden(&mut reading.windows),
            AgentId::Claude => reading.reset_credits = None,
            AgentId::Antigravity | AgentId::Cursor => {}
        }
        ProviderLimitsDto {
            provider: self.provider,
            status: LimitsStatus::Ok,
            message: None,
            account: Some(self.account),
            current_account: false,
            saved_profile: false,
            archived: false,
            reading,
        }
    }
}

pub struct Remembered {
    pub card: ProviderLimitsDto,
    pub saved: Result<(), String>,
}

fn read_stored(path: &Path) -> Option<StoredSnapshot> {
    fs::read_to_string(path).ok().and_then(|raw| decode(&raw))
}

fn write_over(
    path: &Path,
    dto: &ProviderLimitsDto,
    existing: Option<&StoredSnapshot>,
) -> Result<(), String> {
    if !dto.reading.has_observations() {
        return Err("snapshot has no observations".to_string());
    }
    let incoming_latest = newest(&dto.reading.windows)
        .or_else(|| (dto.status == LimitsStatus::Ok && dto.reading.has_figures()).then(Utc::now));
    let incoming_latest =
        incoming_latest.ok_or_else(|| "snapshot has no dated observations".to_string())?;
    if existing
        .and_then(StoredSnapshot::latest_observed_at)
        .is_some_and(|existing| existing > incoming_latest)
    {
        return Ok(());
    }
    write_stored(path, StoredSnapshot::from_dto(dto, incoming_latest))
}

fn decode(raw: &str) -> Option<StoredSnapshot> {
    let stored: StoredSnapshot = serde_json::from_str(raw).ok()?;
    (stored.schema_version == SNAPSHOT_SCHEMA_VERSION).then_some(stored)
}

fn write_stored(path: &Path, stored: StoredSnapshot) -> Result<(), String> {
    let json = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
    atomic_write(path, &json).map_err(|error| format!("{}: {error}", path.display()))
}

fn is_snapshot_file(provider: AgentId, name: &str) -> bool {
    name.starts_with(&format!("{}-", provider.key())) && name.ends_with(".json")
}

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

fn supersedes(scoped: &ProviderLimitsDto, legacy: &ProviderLimitsDto) -> bool {
    !legacy.current_account
        && scoped.provider == legacy.provider
        && scoped.status == LimitsStatus::Ok
        && matches!(
            (&scoped.account, &legacy.account),
            (Some(new), Some(old)) if replaces(new, old)
        )
}

fn replaces(scoped: &LimitsAccountDto, legacy: &LimitsAccountDto) -> bool {
    if !scoped.id.starts_with("profile:")
        || legacy.id.starts_with("profile:")
        || scoped.legacy_id.as_deref() != Some(legacy.id.as_str())
    {
        return false;
    }
    match (&scoped.label, &legacy.label) {
        (Some(new), Some(old)) => {
            !new.trim().is_empty() && new.trim().eq_ignore_ascii_case(old.trim())
        }
        _ => false,
    }
}

pub(super) fn without_superseded(accounts: Vec<ProviderLimitsDto>) -> Vec<ProviderLimitsDto> {
    let hidden: Vec<bool> = accounts
        .iter()
        .map(|old| accounts.iter().any(|new| supersedes(new, old)))
        .collect();
    accounts
        .into_iter()
        .zip(hidden)
        .filter_map(|(dto, hidden)| (!hidden).then_some(dto))
        .collect()
}

#[cfg(test)]
mod tests;
