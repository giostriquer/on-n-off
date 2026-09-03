use chrono::{DateTime, Utc};

use super::*;
use crate::dto::{
    AgentId, LimitWindowDto, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto,
};
use crate::paths::scratch_dir;
use std::fs::{self, OpenOptions};
use std::io::Write as _;

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
fn session_reconciliation_never_reads_an_unbounded_file_prefix() {
    let home = scratch_dir("limits-codex-session-bounded-tail");
    let sessions = home.join(".codex/sessions/2026/08/20");
    fs::create_dir_all(&sessions).unwrap();
    let mut content = String::from(
        r#"{"timestamp":"2026-08-20T06:58:07.093Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":96.0,"window_minutes":10080,"resets_at":1787614473}}}}"#,
    );
    content.push('\n');
    content.push_str(&"x".repeat(300 * 1024));
    fs::write(sessions.join("rollout.jsonl"), content).unwrap();
    let mut accounts = vec![remembered("acct-old", "2026-08-24T23:34:33Z")];
    let now = DateTime::parse_from_rfc3339("2026-08-20T14:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    let updates = merge_recent(&home, now, &mut accounts);

    assert_eq!(updates, 0);
    assert_eq!(accounts[0].windows[0].used_percent, 86.0);
}

#[test]
fn session_reconciliation_reads_a_limit_event_inside_the_bounded_tail() {
    let home = scratch_dir("limits-codex-session-tail-event");
    let sessions = home.join(".codex/sessions/2026/08/20");
    fs::create_dir_all(&sessions).unwrap();
    let mut content = "x".repeat(300 * 1024);
    content.push('\n');
    content.push_str(
        r#"{"timestamp":"2026-08-20T06:58:07.093Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":96.0,"window_minutes":10080,"resets_at":1787614473}}}}"#,
    );
    fs::write(sessions.join("rollout.jsonl"), content).unwrap();
    let mut accounts = vec![remembered("acct-old", "2026-08-24T23:34:33Z")];
    let now = DateTime::parse_from_rfc3339("2026-08-20T14:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    let updates = merge_recent(&home, now, &mut accounts);

    assert_eq!(updates, 1);
    assert_eq!(accounts[0].windows[0].used_percent, 96.0);
}

#[test]
fn a_tail_reader_stops_at_the_snapshotted_end_while_preserving_existing_events() {
    let home = scratch_dir("limits-codex-session-snapshot-range");
    let path = home.join("rollout.jsonl");
    let mut content = "x".repeat(300 * 1024);
    content.push('\n');
    content.push_str(
        r#"{"timestamp":"2026-08-20T06:58:07.093Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":96.0,"window_minutes":10080,"resets_at":1787614473}}}}"#,
    );
    content.push('\n');
    fs::write(&path, content).unwrap();
    let file = File::open(&path).unwrap();
    let length = file.metadata().unwrap().len();
    let start = length.saturating_sub(SESSION_FILE_TAIL_BYTES);
    let mut append = OpenOptions::new().append(true).open(&path).unwrap();
    append
        .write_all(
            br#"{"timestamp":"2026-08-20T07:58:07.093Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":99.0,"window_minutes":10080,"resets_at":1787614473}}}}
"#,
        )
        .unwrap();
    let now = DateTime::parse_from_rfc3339("2026-08-20T14:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let mut observations = Vec::new();

    read_file_range(
        file,
        start,
        length - start,
        now - Duration::days(LOOKBACK_DAYS),
        now,
        &mut observations,
    );

    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].used_percent, 96.0);
}

#[test]
fn session_reconciliation_has_a_bounded_file_count() {
    let home = scratch_dir("limits-codex-session-bounded-files");
    let sessions = home.join(".codex/sessions/2026/08/20");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(
        sessions.join("rollout-0000.jsonl"),
        r#"{"timestamp":"2026-08-20T06:58:07.093Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":96.0,"window_minutes":10080,"resets_at":1787614473}}}}"#,
    )
    .unwrap();
    for index in 1..=512 {
        fs::write(sessions.join(format!("rollout-{index:04}.jsonl")), "{}\n").unwrap();
    }
    let mut accounts = vec![remembered("acct-old", "2026-08-24T23:34:33Z")];
    let now = DateTime::parse_from_rfc3339("2026-08-20T14:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    let updates = merge_recent(&home, now, &mut accounts);

    assert_eq!(updates, 0);
    assert_eq!(accounts[0].windows[0].used_percent, 86.0);
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
