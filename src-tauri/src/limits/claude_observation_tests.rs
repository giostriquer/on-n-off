use super::*;
use crate::dto::LimitWindowKind;
use crate::paths::scratch_dir;
use http::{refused_url, serve_once};
use json::window;
use std::fs;

const EXPIRED_RENEWABLE_LOGIN: &str = r#"{"claudeAiOauth":{"accessToken":"kc-token","expiresAt":1787022473402,"refreshToken":"rt","refreshTokenExpiresAt":1787981634215,"subscriptionType":"max"}}"#;
const DESKTOP_TIMESTAMP_MS: i64 = 1787022473402;

struct ClaudeObservationRig {
    home: PathBuf,
    memo: ClaudeLoginMemo,
}

impl ClaudeObservationRig {
    fn new() -> Self {
        let home = scratch_dir("limits-claude-desktop");
        write(
            &home.join(".claude").join(".credentials.json"),
            EXPIRED_RENEWABLE_LOGIN,
        );
        write(
            &home.join(".claude.json"),
            r#"{"oauthAccount":{"accountUuid":"uuid-1","emailAddress":"me@example.com","organizationUuid":"org-1"}}"#,
        );
        write(
            &claude_desktop::history_path_for_home(&home),
            r#"{"version":2,"samples":[{"t":1787022473402,"org":"org-1","u":{"fh":17,"sd":63}}]}"#,
        );
        Self {
            home,
            memo: ClaudeLoginMemo::new(),
        }
    }

    fn remember(&self, observed_at: &str, used_percent: f64) {
        SnapshotStore::for_home(&self.home)
            .save(&ProviderLimitsDto {
                provider: AgentId::Claude,
                status: LimitsStatus::Ok,
                message: None,
                account: Some(LimitsAccountDto {
                    id: "uuid-1".to_string(),
                    label: Some("me@example.com".to_string()),
                }),
                current_account: true,
                plan: Some("pro".to_string()),
                windows: vec![LimitWindowDto {
                    observed_at: observed_at.to_string(),
                    ..window(
                        "weekly_all",
                        "Weekly · all models",
                        LimitWindowKind::Weekly,
                        used_percent,
                        None,
                    )
                }],
                credits: None,
            })
            .unwrap();
    }

    fn read(&self) -> Vec<ProviderLimitsDto> {
        read_limits_in(
            AgentId::Claude,
            false,
            Sources {
                home: &self.home,
                memo: &self.memo,
                keychain: || Ok(None),
                claude_url: &refused_url(),
                codex_url: &refused_url(),
                claude_desktop_history: claude_desktop::history_path_for_home(&self.home),
                now_ms: DESKTOP_TIMESTAMP_MS + 1,
            },
        )
    }
}

fn write(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn newer_same_organization_observations_keep_one_source_neutral_claude_account() {
    let rig = ClaudeObservationRig::new();
    rig.remember("2026-08-17T15:00:00.000Z", 39.0);

    let dtos = rig.read();

    assert_eq!(dtos.len(), 1, "one card per Claude Code account");
    assert_eq!(dtos[0].status, LimitsStatus::Unauthenticated);
    assert!(dtos[0].current_account);
    assert_eq!(
        dtos[0].message.as_deref(),
        Some("Access token expired — send a prompt with `claude` to renew it, then refresh here.")
    );
    assert_eq!(dtos[0].plan.as_deref(), Some("pro"));
    assert_eq!(
        dtos[0]
            .windows
            .iter()
            .map(|window| (
                window.id.as_str(),
                window.kind,
                window.used_percent,
                window.resets_at.as_deref()
            ))
            .collect::<Vec<_>>(),
        [
            ("weekly_all", LimitWindowKind::Weekly, 63.0, None),
            ("session", LimitWindowKind::Session, 17.0, None),
        ]
    );
    assert!(dtos[0]
        .windows
        .iter()
        .all(|window| window.observed_at == "2026-08-18T03:07:53.402Z"));
}

#[test]
fn newer_on_n_off_usage_wins_without_a_desktop_source_note() {
    let rig = ClaudeObservationRig::new();
    rig.remember("2026-08-19T00:00:00.000Z", 39.0);

    let dtos = rig.read();

    let weekly = dtos[0]
        .windows
        .iter()
        .find(|window| window.id == "weekly_all")
        .unwrap();
    let session = dtos[0]
        .windows
        .iter()
        .find(|window| window.id == "session")
        .unwrap();
    assert_eq!(weekly.used_percent, 39.0);
    assert_eq!(weekly.observed_at, "2026-08-19T00:00:00.000Z");
    assert_eq!(session.used_percent, 17.0);
    assert_eq!(session.observed_at, "2026-08-18T03:07:53.402Z");
    assert!(!dtos[0]
        .message
        .as_deref()
        .unwrap()
        .contains("Showing Claude Desktop usage"));
}

#[test]
fn freshness_compares_instants_instead_of_timestamp_text() {
    let rig = ClaudeObservationRig::new();
    rig.remember("2026-08-18T04:00:00.000+01:00", 39.0);

    let dtos = rig.read();

    let weekly = dtos[0]
        .windows
        .iter()
        .find(|window| window.id == "weekly_all")
        .unwrap();
    assert_eq!(weekly.used_percent, 63.0);
    assert_eq!(weekly.observed_at, "2026-08-18T03:07:53.402Z");
    assert!(!dtos[0].message.as_deref().unwrap().contains("Desktop"));
}

#[test]
fn a_successful_endpoint_read_still_merges_and_persists_other_local_windows() {
    let rig = ClaudeObservationRig::new();
    write(
        &rig.home.join(".claude").join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"token","expiresAt":1787023473402,"refreshToken":"rt","refreshTokenExpiresAt":1787981634215,"subscriptionType":"max"}}"#,
    );
    let (url, request) = serve_once(
        "200 OK",
        r#"{"seven_day":{"utilization":50,"resets_at":"2026-08-24T13:59:59Z"}}"#,
    );

    let dtos = read_limits_in(
        AgentId::Claude,
        false,
        Sources {
            home: &rig.home,
            memo: &rig.memo,
            keychain: || Ok(None),
            claude_url: &url,
            codex_url: &refused_url(),
            claude_desktop_history: claude_desktop::history_path_for_home(&rig.home),
            now_ms: DESKTOP_TIMESTAMP_MS + 1,
        },
    );
    request.join().unwrap();

    assert_eq!(dtos[0].status, LimitsStatus::Ok);
    assert_eq!(dtos[0].windows.len(), 2);
    assert_eq!(dtos[0].windows[0].id, "weekly_all");
    assert_eq!(dtos[0].windows[0].used_percent, 50.0);
    assert_eq!(dtos[0].windows[1].id, "session");
    assert_eq!(dtos[0].windows[1].used_percent, 17.0);
    let persisted = SnapshotStore::for_home(&rig.home).load(AgentId::Claude);
    assert_eq!(persisted[0].windows.len(), 2);
    assert_eq!(persisted[0].windows[1].id, "session");
}
