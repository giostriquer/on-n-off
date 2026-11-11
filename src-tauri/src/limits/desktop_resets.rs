use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use chrono::{DateTime, SecondsFormat};
use ruzstd::decoding::StreamingDecoder;
use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::dto::{AgentId, ColumnEntryDto, DesktopResetsDto};
use crate::paths;
use crate::provider_catalogue::DesktopApp;

const ENTRY_MAGIC: u64 = 0xfcfb_6d1b_a772_5c30;
const HEADER_BYTES: usize = 24;
const KEY_LENGTH_AT: usize = 12;
const MAX_ENTRY_BYTES: u64 = 512 * 1024;
const MAX_BODY_BYTES: u64 = 256 * 1024;
const MAX_RESETS_PER_GRANT: u64 = 100;

pub(crate) fn attach_desktop_resets(
    provider: AgentId,
    column: &mut [ColumnEntryDto],
    home: &Path,
    now_ms: i64,
) {
    let Some(app) = provider.limits_facts().desktop_app else {
        return;
    };
    let cache = match app {
        DesktopApp::Claude => paths::claude_desktop_data_for(home)
            .join("Cache")
            .join("Cache_Data"),
    };
    let organizations: Vec<Option<String>> = column
        .iter()
        .map(|entry| organization_of(app, entry, home))
        .collect();
    let mut accounts: BTreeMap<&str, usize> = BTreeMap::new();
    for organization in organizations.iter().flatten() {
        *accounts.entry(organization).or_default() += 1;
    }
    for (entry, organization) in column.iter_mut().zip(&organizations) {
        entry.desktop_resets = organization
            .as_deref()
            .filter(|organization| accounts.get(organization) == Some(&1))
            .and_then(|organization| read(&cache, organization, now_ms));
    }
}

fn organization_of(app: DesktopApp, entry: &ColumnEntryDto, home: &Path) -> Option<String> {
    match app {
        DesktopApp::Claude => match &entry.profile {
            Some(profile) => Some(profile.identity().workspace_id.clone()),
            None if entry.card.current_account => {
                let claude = crate::accounts::claude::SignedIn::resolve(home).ok()?;
                super::claude_config::read_claude_config_account(claude.config_file())?
                    .identity
                    .organization_id
            }
            None => None,
        },
    }
}

fn usage_key(organization: &str) -> String {
    format!(
        "1/0/https://claude.ai/api/organizations/{organization}/usage?cedar_ember=1&skip_spend=1"
    )
}

fn entry_file_name(key: &str) -> String {
    let digest = Sha1::digest(key.as_bytes());
    let mut first = [0; 8];
    first.copy_from_slice(&digest[..8]);
    format!("{:016x}_0", u64::from_le_bytes(first))
}

fn read(cache: &Path, organization: &str, now_ms: i64) -> Option<DesktopResetsDto> {
    let key = usage_key(organization);
    let file = std::fs::File::open(cache.join(entry_file_name(&key))).ok()?;
    let written = file.metadata().ok()?.modified().ok();
    let mut entry = Vec::new();
    file.take(MAX_ENTRY_BYTES + 1)
        .read_to_end(&mut entry)
        .ok()?;
    if entry.len() as u64 > MAX_ENTRY_BYTES {
        return None;
    }
    let answer = Answer::in_entry(&entry, &key)?;
    let observed_at_ms = answer.sent_at_ms.or_else(|| {
        let written = written?.duration_since(std::time::UNIX_EPOCH).ok()?;
        i64::try_from(written.as_millis()).ok()
    })?;
    let (available_count, next_expires_at_ms) = usable_resets(&answer.body, now_ms)?;
    Some(DesktopResetsDto {
        observed_at: spelled(observed_at_ms)?,
        available_count,
        next_expires_at: next_expires_at_ms.and_then(spelled),
    })
}

struct Answer {
    body: Value,
    sent_at_ms: Option<i64>,
}

impl Answer {
    fn in_entry(entry: &[u8], key: &str) -> Option<Self> {
        let magic = u64::from_le_bytes(entry.get(..8)?.try_into().ok()?);
        if magic != ENTRY_MAGIC {
            return None;
        }
        let key_length = entry.get(KEY_LENGTH_AT..KEY_LENGTH_AT + 4)?;
        let key_length = usize::try_from(u32::from_le_bytes(key_length.try_into().ok()?)).ok()?;
        let key_end = HEADER_BYTES.checked_add(key_length)?;
        if entry.get(HEADER_BYTES..key_end)? != key.as_bytes() {
            return None;
        }
        let mut after_key = entry.get(key_end..)?;
        let mut body = Vec::new();
        StreamingDecoder::new(&mut after_key)
            .ok()?
            .take(MAX_BODY_BYTES + 1)
            .read_to_end(&mut body)
            .ok()?;
        if body.len() as u64 > MAX_BODY_BYTES {
            return None;
        }
        Some(Self {
            body: serde_json::from_slice(&body).ok()?,
            sent_at_ms: sent_at_ms(after_key),
        })
    }
}

fn sent_at_ms(response_headers: &[u8]) -> Option<i64> {
    [b"\0date:".as_slice(), b"\0Date:".as_slice()]
        .into_iter()
        .find_map(|name| {
            let at = response_headers
                .windows(name.len())
                .position(|window| window == name)?;
            let value = response_headers.get(at + name.len()..)?;
            let value = value.split(|byte| *byte == 0).next()?;
            let sent =
                DateTime::parse_from_rfc2822(std::str::from_utf8(value).ok()?.trim()).ok()?;
            Some(sent.timestamp_millis())
        })
}

fn usable_resets(answer: &Value, now_ms: i64) -> Option<(u32, Option<i64>)> {
    let program = answer.get("cedar_ember")?;
    if program.get("eligible").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let grants: Vec<(u32, Option<i64>)> = program
        .get("grants")?
        .as_array()?
        .iter()
        .filter_map(|grant| usable_grant(grant, now_ms))
        .collect();
    let left: u32 = grants.iter().map(|(left, _)| left).sum();
    let next_ends_at_ms = grants
        .iter()
        .filter_map(|(_, ends_at_ms)| *ends_at_ms)
        .min();
    (left > 0).then_some((left, next_ends_at_ms))
}

fn usable_grant(grant: &Value, now_ms: i64) -> Option<(u32, Option<i64>)> {
    let left = grant.get("resets_left")?.as_u64()?;
    if left == 0
        || left > MAX_RESETS_PER_GRANT
        || grant.get("paused").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let starts_at_ms = instant(grant.get("starts_at"))?;
    let ends_at_ms = instant(grant.get("ends_at"))?;
    if starts_at_ms.is_some_and(|starts| starts > now_ms)
        || ends_at_ms.is_some_and(|ends| ends <= now_ms)
    {
        return None;
    }
    Some((u32::try_from(left).ok()?, ends_at_ms))
}

fn instant(value: Option<&Value>) -> Option<Option<i64>> {
    match value {
        None | Some(Value::Null) => Some(None),
        Some(Value::String(at)) => DateTime::parse_from_rfc3339(at)
            .ok()
            .map(|at| Some(at.timestamp_millis())),
        Some(_) => None,
    }
}

fn spelled(at_ms: i64) -> Option<String> {
    DateTime::from_timestamp_millis(at_ms).map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, true))
}

#[cfg(test)]
mod tests;
