//! Remembered per-account limit snapshots under `<home>/.on-n-off/limits/`.
//!
//! The CLIs store one login at a time, so switching accounts (`codex login`, `claude`) makes the
//! previous account invisible. Canonical per-window observations are written here (numbers only —
//! never a token) so the screen can keep showing each account's last observations.

use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use super::reading::{keep_remembered, newest, parse_observed_at};
use crate::dto::{AgentId, LimitsAccountDto, LimitsStatus, ProviderLimitsDto, Reading};
use crate::usage::cache_io::atomic_write;

const SNAPSHOT_SCHEMA_VERSION: u8 = 2;
// Sign-in and the live provider reader can publish concurrently. Protect the timestamp check
// and replacement together; this lock covers only local snapshot I/O, never provider calls.
static SNAPSHOT_WRITES: Mutex<()> = Mutex::new(());

/// One account's remembered reading as its file holds it: the reading's own keys beside the
/// account, the schema version and the time that dates it. The live offer is never among them.
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

    /// `card`, keeping what its read could not tell from what its account's file remembers now, by
    /// the remember policy's column for how the read went ([`keep_remembered`]); then written over
    /// that file when it observed something datable and is not older than it. The card comes back
    /// kept either way, beside whether the file was written.
    ///
    /// Every read goes through this once: the signed-in read (`aggregate_accounts`), a saved
    /// profile's poll and the first usage after a sign-in (`accounts/`).
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

    /// Persist `dto` as it is: canonical account observations. Dated local or remembered windows
    /// remain trustworthy while refresh is unavailable; a successful read with only credits or banked
    /// resets is dated when it reaches this storage boundary. A file with newer observations is
    /// left alone.
    pub fn save(&self, dto: &ProviderLimitsDto) -> Result<(), String> {
        let _write = SNAPSHOT_WRITES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = self
            .path_of(dto)
            .ok_or_else(|| "snapshot has no account".to_string())?;
        write_over(&path, dto, read_stored(&path).as_ref())
    }

    /// Where the file of `dto`'s account is; `None` for a card that names no account.
    fn path_of(&self, dto: &ProviderLimitsDto) -> Option<PathBuf> {
        let account = dto.account.as_ref()?;
        Some(self.dir.join(file_name(dto.provider, &account.id)))
    }

    /// Persist the accounts a merge changed, leaving the others' files and dates alone: re-saving an
    /// untouched account with no dated windows would date its old figures now.
    pub fn save_changed(&self, before: &[ProviderLimitsDto], after: &[ProviderLimitsDto]) {
        for (changed, previous) in after.iter().zip(before) {
            if changed != previous {
                let _ = self.save(changed);
            }
        }
    }

    /// The remembered snapshots the cards show for `provider`, newest first: `stored` less
    /// any left with nothing observed once a lapsed banked-reset count is dropped (as `save` would
    /// refuse to write it, and which so no longer hides the legacy history it superseded), less the
    /// legacy history a remaining scoped observation supersedes.
    pub fn load(&self, provider: AgentId) -> Vec<ProviderLimitsDto> {
        without_superseded(
            self.stored(provider)
                .into_iter()
                .filter(|dto| dto.reading.has_observations())
                .collect(),
        )
    }

    /// Every readable snapshot for `provider`, newest first, whatever it still observes. Unreadable
    /// files are skipped rather than failing the whole read, and files from obsolete snapshot
    /// schemas are ignored. Forget works from this list, so an account whose count lapsed still
    /// takes the history it replaced.
    fn stored(&self, provider: AgentId) -> Vec<ProviderLimitsDto> {
        let now = Utc::now();
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
                Some((stored.latest_observed_at(), stored.into_dto(now)))
            })
            .filter(|(_, dto)| dto.provider == provider)
            .collect();
        snapshots.sort_by_key(|(observed_at, _)| Reverse(*observed_at));
        snapshots.into_iter().map(|(_, dto)| dto).collect()
    }

    /// Conditionally remove legacy history identified by a confirmed saved-account card.
    /// Recheck the stored label: a workspace-only key may have been reused by another user.
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
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
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
        self.remove_file(provider, account_id)
    }

    /// Delete one account's snapshot; unknown accounts are a no-op.
    pub fn forget(&self, provider: AgentId, account_id: &str) -> Result<(), String> {
        let _write = SNAPSHOT_WRITES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let all = self.stored(provider);
        if let Some(target) = all
            .iter()
            .find(|dto| dto.account.as_ref().is_some_and(|a| a.id == account_id))
        {
            for legacy in all.iter().filter(|dto| supersedes(target, dto)) {
                self.remove_file(
                    provider,
                    &legacy.account.as_ref().expect("matched account").id,
                )?;
            }
        }
        self.remove_file(provider, account_id)
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
            // A live offer belongs to the read that saw it and is never remembered.
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

    /// The card a remembered reading shows at `now`. A Codex file written before the reader dropped
    /// hidden windows loses them here, by the reader's own rule; the file loses them at its next save.
    fn into_dto(self, now: DateTime<Utc>) -> ProviderLimitsDto {
        let mut reading = self.reading.known_at(now);
        if self.provider == AgentId::Codex {
            super::codex::drop_hidden(&mut reading.windows);
        }
        ProviderLimitsDto {
            provider: self.provider,
            status: LimitsStatus::Ok,
            message: None,
            account: Some(self.account),
            current_account: false,
            saved_profile: false,
            reading,
        }
    }
}

/// What [`SnapshotStore::remember`] made of a card.
pub struct Remembered {
    /// The card to show: the read, with what it could not tell kept from the account's file.
    pub card: ProviderLimitsDto,
    /// Whether the file holds it: `Ok` also when the file already held newer observations, `Err`
    /// when the card observed nothing datable or the write failed.
    pub saved: Result<(), String>,
}

/// The snapshot at `path`, when there is a readable one of this schema.
fn read_stored(path: &Path) -> Option<StoredSnapshot> {
    fs::read_to_string(path).ok().and_then(|raw| decode(&raw))
}

/// Write `dto` over `existing`, the file at `path`, unless it observed nothing datable or the file
/// is newer.
fn write_over(
    path: &Path,
    dto: &ProviderLimitsDto,
    existing: Option<&StoredSnapshot>,
) -> Result<(), String> {
    if !dto.reading.has_observations() {
        return Err("snapshot has no observations".to_string());
    }
    let incoming_latest = newest(&dto.reading.windows).or_else(|| {
        // Figures have no observation time of their own; a successful read dates them here.
        (dto.status == LimitsStatus::Ok && dto.reading.has_figures()).then(Utc::now)
    });
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

/// New scoped observations supersede the old card only when both its provider-specific legacy
/// key and email match. Keep the old file until Forget; never import its unscoped quota windows.
fn supersedes(scoped: &ProviderLimitsDto, legacy: &ProviderLimitsDto) -> bool {
    if legacy.current_account
        || scoped.provider != legacy.provider
        || scoped.status != LimitsStatus::Ok
    {
        return false;
    }
    let (Some(new), Some(old)) = (&scoped.account, &legacy.account) else {
        return false;
    };
    if !new.id.starts_with("profile:")
        || old.id.starts_with("profile:")
        || new.legacy_id.as_deref() != Some(old.id.as_str())
    {
        return false;
    }
    match (&new.label, &old.label) {
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
