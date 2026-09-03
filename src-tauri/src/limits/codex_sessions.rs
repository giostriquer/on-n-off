//! Read-only Codex rate-limit observations from local session event streams.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Duration, SecondsFormat, Utc};
use serde::Deserialize;

use crate::dto::{LimitWindowKind, ProviderLimitsDto};

const LOOKBACK_DAYS: i64 = 8;
// Codex emits rate-limit samples repeatedly. The newest 256 KiB keeps each session's latest
// samples, while 512 files cap one reconciliation at 128 MiB even when transcripts span gigabytes.
const MAX_SESSION_FILES: usize = 512;
const SESSION_FILE_TAIL_BYTES: u64 = 256 * 1024;
const RESET_TOLERANCE_SECONDS: i64 = 2;
const WEEKLY_THRESHOLD_SECONDS: u64 = 24 * 60 * 60;

struct SessionWindow {
    id: &'static str,
    kind: LimitWindowKind,
    used_percent: f64,
    resets_at: DateTime<Utc>,
    window_seconds: u64,
    observed_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct EventEnvelope {
    #[serde(rename = "type")]
    event_type: Option<String>,
    timestamp: Option<String>,
    payload: Option<TokenCountPayload>,
}

#[derive(Deserialize)]
struct TokenCountPayload {
    #[serde(rename = "type")]
    payload_type: Option<String>,
    rate_limits: Option<SessionRateLimits>,
}

#[derive(Deserialize)]
struct SessionRateLimits {
    limit_id: Option<String>,
    primary: Option<SessionRateWindow>,
    secondary: Option<SessionRateWindow>,
}

#[derive(Deserialize)]
struct SessionRateWindow {
    used_percent: Option<f64>,
    window_minutes: Option<u64>,
    resets_at: Option<i64>,
}

pub(super) fn merge_recent(
    home: &Path,
    now: DateTime<Utc>,
    accounts: &mut [ProviderLimitsDto],
) -> usize {
    let mut observations = read_recent(home, now);
    observations.sort_by_key(|observation| observation.observed_at);
    let mut updates = 0;
    for observation in observations {
        let matches: Vec<(usize, usize)> = accounts
            .iter()
            .enumerate()
            .filter(|(_, account)| !account.current_account)
            .flat_map(|(account_index, account)| {
                account
                    .windows
                    .iter()
                    .enumerate()
                    .filter(|(_, window)| matches_window(window, &observation))
                    .map(move |(window_index, _)| (account_index, window_index))
            })
            .collect();
        if matches.len() != 1 {
            continue;
        }
        let (account_index, window_index) = matches[0];
        let window = &mut accounts[account_index].windows[window_index];
        window.used_percent = observation.used_percent;
        window.resets_at = Some(
            observation
                .resets_at
                .to_rfc3339_opts(SecondsFormat::Secs, true),
        );
        window.window_seconds = Some(observation.window_seconds);
        window.observed_at = observation
            .observed_at
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        updates += 1;
    }
    updates
}

fn read_recent(home: &Path, now: DateTime<Utc>) -> Vec<SessionWindow> {
    let mut observations = Vec::new();
    let mut scanned_files = 0;
    for offset in 0..=LOOKBACK_DAYS {
        let day = (now - Duration::days(offset)).date_naive();
        let dir = home
            .join(".codex")
            .join("sessions")
            .join(format!("{:04}", day.year()))
            .join(format!("{:02}", day.month()))
            .join(format!("{:02}", day.day()));
        for path in jsonl_files(&dir) {
            if scanned_files == MAX_SESSION_FILES {
                return observations;
            }
            read_file(
                &path,
                now - Duration::days(LOOKBACK_DAYS),
                now,
                &mut observations,
            );
            scanned_files += 1;
        }
    }
    observations
}

fn jsonl_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    paths.sort_unstable_by(|left, right| right.cmp(left));
    paths
}

fn read_file(
    path: &Path,
    cutoff: DateTime<Utc>,
    now: DateTime<Utc>,
    observations: &mut Vec<SessionWindow>,
) {
    let Ok(file) = File::open(path) else {
        return;
    };
    let Ok(length) = file.metadata().map(|metadata| metadata.len()) else {
        return;
    };
    let start = length.saturating_sub(SESSION_FILE_TAIL_BYTES);
    read_file_range(file, start, length - start, cutoff, now, observations);
}

fn read_file_range(
    mut file: File,
    start: u64,
    length: u64,
    cutoff: DateTime<Utc>,
    now: DateTime<Utc>,
    observations: &mut Vec<SessionWindow>,
) {
    let starts_at_line_boundary = if start == 0 {
        true
    } else {
        let mut previous = [0];
        file.seek(SeekFrom::Start(start - 1)).is_ok()
            && file.read_exact(&mut previous).is_ok()
            && previous[0] == b'\n'
    };
    if file.seek(SeekFrom::Start(start)).is_err() {
        return;
    }
    let mut reader = BufReader::new(file.take(length));
    if !starts_at_line_boundary {
        let mut partial = Vec::new();
        if reader.read_until(b'\n', &mut partial).is_err() {
            return;
        }
    }
    for line in reader.lines().map_while(Result::ok) {
        if !line.contains("\"token_count\"") || !line.contains("\"rate_limits\"") {
            continue;
        }
        observations.extend(parse_event(&line).into_iter().filter(|observation| {
            observation.observed_at >= cutoff && observation.observed_at <= now
        }));
    }
}

fn parse_event(line: &str) -> Vec<SessionWindow> {
    let Ok(event) = serde_json::from_str::<EventEnvelope>(line) else {
        return Vec::new();
    };
    if event.event_type.as_deref() != Some("event_msg")
        || event
            .payload
            .as_ref()
            .and_then(|payload| payload.payload_type.as_deref())
            != Some("token_count")
    {
        return Vec::new();
    }
    let Some(timestamp) = event
        .timestamp
        .as_deref()
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc))
    else {
        return Vec::new();
    };
    let Some(limits) = event.payload.and_then(|payload| payload.rate_limits) else {
        return Vec::new();
    };
    if limits.limit_id.as_deref() != Some("codex") {
        return Vec::new();
    }
    [(limits.primary, "primary"), (limits.secondary, "secondary")]
        .into_iter()
        .filter_map(|(window, id)| parse_window(window.as_ref()?, id, timestamp))
        .collect()
}

fn parse_window(
    value: &SessionRateWindow,
    id: &'static str,
    observed_at: DateTime<Utc>,
) -> Option<SessionWindow> {
    let used_percent = value.used_percent?;
    if !used_percent.is_finite() {
        return None;
    }
    let used_percent = used_percent.clamp(0.0, 100.0);
    let window_seconds = value.window_minutes?.checked_mul(60)?;
    let resets_at = value
        .resets_at
        .and_then(|epoch| DateTime::<Utc>::from_timestamp(epoch, 0))?;
    let kind = if window_seconds < WEEKLY_THRESHOLD_SECONDS {
        LimitWindowKind::Session
    } else {
        LimitWindowKind::Weekly
    };
    Some(SessionWindow {
        id,
        kind,
        used_percent,
        resets_at,
        window_seconds,
        observed_at,
    })
}

fn matches_window(window: &crate::dto::LimitWindowDto, observation: &SessionWindow) -> bool {
    if window.id != observation.id || window.kind != observation.kind {
        return false;
    }
    if window.window_seconds != Some(observation.window_seconds) {
        return false;
    }
    let Some(reset) = window
        .resets_at
        .as_deref()
        .and_then(|reset| DateTime::parse_from_rfc3339(reset).ok())
        .map(|reset| reset.with_timezone(&Utc))
    else {
        return false;
    };
    if (reset - observation.resets_at).num_seconds().abs() > RESET_TOLERANCE_SECONDS {
        return false;
    }
    DateTime::parse_from_rfc3339(&window.observed_at)
        .is_ok_and(|existing| observation.observed_at > existing.with_timezone(&Utc))
}

#[cfg(test)]
mod tests;
