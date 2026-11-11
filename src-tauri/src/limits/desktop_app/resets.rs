use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, SecondsFormat};
use serde_json::Value;

use super::simple_cache;
use crate::dto::{ColumnEntryDto, DesktopResetsDto};
use crate::paths;
use crate::provider_catalogue::DesktopApp;

const MAX_RESETS_PER_GRANT: u64 = 100;

pub(super) fn attach(app: DesktopApp, column: &mut [ColumnEntryDto], home: &Path, now_ms: i64) {
    if cfg!(target_os = "macos") {
        attach_from_cache(app, column, home, now_ms);
    }
}

fn attach_from_cache(app: DesktopApp, column: &mut [ColumnEntryDto], home: &Path, now_ms: i64) {
    let cache = match app {
        DesktopApp::Claude => paths::claude_desktop_cache_for(home),
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
            .and_then(|organization| seen(&cache, organization, now_ms));
    }
}

fn organization_of(app: DesktopApp, entry: &ColumnEntryDto, home: &Path) -> Option<String> {
    match app {
        DesktopApp::Claude => match &entry.profile {
            Some(profile) => Some(profile.identity().workspace_id.clone()),
            None if entry.card.current_account => {
                crate::limits::claude::signed_in_organization(home, entry.card.account.as_ref()?)
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

fn seen(cache: &Path, organization: &str, now_ms: i64) -> Option<DesktopResetsDto> {
    let cached = simple_cache::response(cache, &usage_key(organization))?;
    let answer: Value = serde_json::from_slice(&cached.body).ok()?;
    let (available_count, next_ends_at_ms) = usable_resets(&answer, now_ms)?;
    Some(DesktopResetsDto {
        observed_at: spelled(cached.seen_at_ms)?,
        available_count,
        next_expires_at: next_ends_at_ms.and_then(spelled),
    })
}

fn usable_resets(answer: &Value, now_ms: i64) -> Option<(u32, Option<i64>)> {
    let program = answer.get("cedar_ember")?;
    if program.get("eligible").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let grants: Vec<(u32, Bound)> = program
        .get("grants")?
        .as_array()?
        .iter()
        .filter_map(|grant| usable_grant(grant, now_ms))
        .collect();
    let left: u32 = grants.iter().map(|(left, _)| left).sum();
    let next_ends_at_ms = grants.iter().filter_map(|(_, ends)| ends.at_ms()).min();
    (left > 0).then_some((left, next_ends_at_ms))
}

fn usable_grant(grant: &Value, now_ms: i64) -> Option<(u32, Bound)> {
    let left = grant.get("resets_left")?.as_u64()?;
    if left == 0
        || left > MAX_RESETS_PER_GRANT
        || grant.get("paused").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let starts = Bound::of(grant.get("starts_at"))?;
    let ends = Bound::of(grant.get("ends_at"))?;
    if starts.at_ms().is_some_and(|starts| starts > now_ms)
        || ends.at_ms().is_some_and(|ends| ends <= now_ms)
    {
        return None;
    }
    Some((u32::try_from(left).ok()?, ends))
}

#[derive(Clone, Copy)]
enum Bound {
    Open,
    At(i64),
}

impl Bound {
    fn of(value: Option<&Value>) -> Option<Self> {
        match value {
            None | Some(Value::Null) => Some(Self::Open),
            Some(Value::String(at)) => DateTime::parse_from_rfc3339(at)
                .ok()
                .map(|at| Self::At(at.timestamp_millis())),
            Some(_) => None,
        }
    }

    fn at_ms(self) -> Option<i64> {
        match self {
            Self::Open => None,
            Self::At(at_ms) => Some(at_ms),
        }
    }
}

fn spelled(at_ms: i64) -> Option<String> {
    DateTime::from_timestamp_millis(at_ms).map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, true))
}

#[cfg(test)]
pub(super) mod tests;
