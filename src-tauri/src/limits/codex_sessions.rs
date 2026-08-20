//! Read-only Codex rate-limit observations from local session event streams.

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Duration, SecondsFormat, Utc};
use serde::Deserialize;

use crate::dto::{LimitWindowKind, ProviderLimitsDto};

const LOOKBACK_DAYS: i64 = 8;
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
    for offset in 0..=LOOKBACK_DAYS {
        let day = (now - Duration::days(offset)).date_naive();
        let dir = home
            .join(".codex")
            .join("sessions")
            .join(format!("{:04}", day.year()))
            .join(format!("{:02}", day.month()))
            .join(format!("{:02}", day.day()));
        for path in jsonl_files(&dir) {
            read_file(
                &path,
                now - Duration::days(LOOKBACK_DAYS),
                now,
                &mut observations,
            );
        }
    }
    observations
}

fn jsonl_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect()
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
    for line in BufReader::new(file).lines().map_while(Result::ok) {
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
mod tests {
    use chrono::{DateTime, Utc};

    use super::*;
    use crate::dto::{
        AgentId, LimitWindowDto, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto,
    };
    use crate::paths::scratch_dir;
    use std::fs;

    fn remembered(id: &str, reset_at: &str) -> ProviderLimitsDto {
        ProviderLimitsDto {
            provider: AgentId::Codex,
            status: LimitsStatus::Ok,
            message: None,
            account: Some(LimitsAccountDto {
                id: id.to_string(),
                label: Some(format!("{id}@example.com")),
            }),
            current_account: false,
            plan: Some("pro".to_string()),
            windows: vec![LimitWindowDto {
                id: "primary".to_string(),
                label: "Weekly · all models".to_string(),
                kind: LimitWindowKind::Weekly,
                used_percent: 86.0,
                resets_at: Some(reset_at.to_string()),
                window_seconds: Some(604_800),
                observed_at: "2026-08-20T02:39:06.754Z".to_string(),
            }],
            credits: None,
        }
    }

    #[test]
    fn a_newer_session_observation_updates_the_uniquely_matching_remembered_account() {
        let home = scratch_dir("limits-codex-session");
        let sessions = home.join(".codex/sessions/2026/08/20");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("rollout.jsonl"),
            r#"{"timestamp":"2026-08-20T06:58:07.093Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","limit_name":null,"primary":{"used_percent":96.0,"window_minutes":10080,"resets_at":1787614473},"secondary":null}}}"#,
        )
        .unwrap();
        let mut accounts = vec![
            remembered("acct-old", "2026-08-24T23:34:33Z"),
            remembered("acct-other", "2026-08-27T13:56:01Z"),
        ];
        let now = DateTime::parse_from_rfc3339("2026-08-20T14:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let updates = merge_recent(&home, now, &mut accounts);

        assert_eq!(updates, 1);
        assert_eq!(accounts[0].windows[0].used_percent, 96.0);
        assert_eq!(
            accounts[0].windows[0].observed_at,
            "2026-08-20T06:58:07.093Z"
        );
        assert_eq!(accounts[1].windows[0].used_percent, 86.0);
    }

    #[test]
    fn an_ambiguous_session_observation_does_not_change_any_account() {
        let home = scratch_dir("limits-codex-session-ambiguous");
        let sessions = home.join(".codex/sessions/2026/08/20");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("rollout.jsonl"),
            r#"{"timestamp":"2026-08-20T06:58:07.093Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":100.0,"window_minutes":10080,"resets_at":1787614473}}}}"#,
        )
        .unwrap();
        let mut accounts = vec![
            remembered("acct-a", "2026-08-24T23:34:33Z"),
            remembered("acct-b", "2026-08-24T23:34:33Z"),
        ];
        let now = DateTime::parse_from_rfc3339("2026-08-20T14:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let updates = merge_recent(&home, now, &mut accounts);

        assert_eq!(updates, 0);
        assert!(accounts
            .iter()
            .all(|account| account.windows[0].used_percent == 86.0));
    }

    #[test]
    fn a_session_observation_does_not_match_a_window_without_an_exact_duration() {
        let home = scratch_dir("limits-codex-session-missing-duration");
        let sessions = home.join(".codex/sessions/2026/08/20");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("rollout.jsonl"),
            r#"{"timestamp":"2026-08-20T06:58:07.093Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":100.0,"window_minutes":10080,"resets_at":1787614473}}}}"#,
        )
        .unwrap();
        let mut accounts = vec![remembered("acct-a", "2026-08-24T23:34:33Z")];
        accounts[0].windows[0].window_seconds = None;
        let now = DateTime::parse_from_rfc3339("2026-08-20T14:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(merge_recent(&home, now, &mut accounts), 0);
        assert_eq!(accounts[0].windows[0].used_percent, 86.0);
    }

    #[test]
    fn duplicate_matching_windows_in_one_account_are_ambiguous() {
        let home = scratch_dir("limits-codex-session-duplicate-window");
        let sessions = home.join(".codex/sessions/2026/08/20");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("rollout.jsonl"),
            r#"{"timestamp":"2026-08-20T06:58:07.093Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":100.0,"window_minutes":10080,"resets_at":1787614473}}}}"#,
        )
        .unwrap();
        let mut account = remembered("acct-a", "2026-08-24T23:34:33Z");
        account.windows.push(account.windows[0].clone());
        let mut accounts = vec![account];
        let now = DateTime::parse_from_rfc3339("2026-08-20T14:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(merge_recent(&home, now, &mut accounts), 0);
        assert!(accounts[0]
            .windows
            .iter()
            .all(|window| window.used_percent == 86.0));
    }

    #[test]
    fn an_unattributed_session_observation_never_overrides_the_current_account() {
        let home = scratch_dir("limits-codex-session-current-account");
        let sessions = home.join(".codex/sessions/2026/08/20");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("rollout.jsonl"),
            r#"{"timestamp":"2026-08-20T06:58:07.093Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":100.0,"window_minutes":10080,"resets_at":1787614473}}}}"#,
        )
        .unwrap();
        let mut current = remembered("acct-current", "2026-08-24T23:34:33Z");
        current.current_account = true;
        let mut accounts = vec![current];
        let now = DateTime::parse_from_rfc3339("2026-08-20T14:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(merge_recent(&home, now, &mut accounts), 0);
        assert_eq!(accounts[0].windows[0].used_percent, 86.0);
    }
}
