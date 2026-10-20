use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Duration, SecondsFormat, Utc};
use serde::Deserialize;

use super::codex::{main_windows, RateLimitWindow};
use super::window::{QuotaWindow, ResetInstant};
use crate::dto::{LimitWindowDto, ProviderLimitsDto};

const LOOKBACK_DAYS: i64 = 8;
const MAX_SESSION_FILES: usize = 512;
const SESSION_FILE_TAIL_BYTES: u64 = 256 * 1024;

struct SessionWindow {
    window: QuotaWindow,
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
    primary: Option<RateLimitWindow>,
    secondary: Option<RateLimitWindow>,
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
        let observed = observation.window.into_dto();
        let matches: Vec<(usize, usize)> = accounts
            .iter()
            .enumerate()
            .filter(|(_, account)| {
                !account.current_account
                    && account
                        .account
                        .as_ref()
                        .is_none_or(|a| !a.id.starts_with("profile:"))
            })
            .flat_map(|(account_index, account)| {
                account
                    .reading
                    .windows
                    .iter()
                    .enumerate()
                    .filter(|(_, window)| {
                        matches_window(window, &observed, observation.observed_at)
                    })
                    .map(move |(window_index, _)| (account_index, window_index))
            })
            .collect();
        if matches.len() != 1 {
            continue;
        }
        let (account_index, window_index) = matches[0];
        accounts[account_index].reading.windows[window_index] = LimitWindowDto {
            observed_at: observation
                .observed_at
                .to_rfc3339_opts(SecondsFormat::Millis, true),
            ..observed
        };
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
    main_windows(limits.primary.as_ref(), limits.secondary.as_ref())
        .into_iter()
        .filter(|window| window.length.is_some() && window.reset.is_some())
        .map(|window| SessionWindow {
            window,
            observed_at: timestamp,
        })
        .collect()
}

fn matches_window(
    window: &LimitWindowDto,
    observed: &LimitWindowDto,
    observed_at: DateTime<Utc>,
) -> bool {
    window.id == observed.id
        && window.kind == observed.kind
        && window.window_seconds == observed.window_seconds
        && ResetInstant::of(window)
            .zip(ResetInstant::of(observed))
            .is_some_and(|(remembered, seen)| remembered.same_cycle_as(seen))
        && DateTime::parse_from_rfc3339(&window.observed_at)
            .is_ok_and(|existing| observed_at > existing.with_timezone(&Utc))
}

#[cfg(test)]
mod tests;
