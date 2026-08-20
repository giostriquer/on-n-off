use super::*;
use crate::paths::scratch_dir;
use http::refused_url;
use json::window;
use std::fs;

const EXPIRED_RENEWABLE_LOGIN: &str = r#"{"claudeAiOauth":{"accessToken":"kc-token","expiresAt":1787022473402,"refreshToken":"rt","refreshTokenExpiresAt":1787981634215,"subscriptionType":"max"}}"#;
const DESKTOP_TIMESTAMP_MS: i64 = 1787022473402;

struct DesktopFallbackRig {
    home: PathBuf,
    memo: ClaudeLoginMemo,
}

impl DesktopFallbackRig {
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

    fn remember(&self, fetched_at: &str, used_percent: f64) {
        SnapshotStore::for_home(&self.home)
            .save(&ProviderLimitsDto {
                provider: AgentId::Claude,
                status: LimitsStatus::Ok,
                message: None,
                account: Some(LimitsAccountDto {
                    id: "uuid-1".to_string(),
                    label: Some("me@example.com".to_string()),
                }),
                live: true,
                plan: Some("pro".to_string()),
                windows: vec![window(
                    "primary",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    used_percent,
                    None,
                )],
                credits: None,
                fetched_at: fetched_at.to_string(),
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
fn newer_matching_desktop_usage_is_used_for_an_expired_claude_code_login() {
    let rig = DesktopFallbackRig::new();
    rig.remember("2026-08-17T15:00:00.000Z", 39.0);

    let dtos = rig.read();

    assert_eq!(dtos.len(), 1, "one card per Claude Code account");
    assert_eq!(dtos[0].status, LimitsStatus::Unauthenticated);
    assert!(dtos[0].live);
    assert!(dtos[0]
        .message
        .as_deref()
        .unwrap()
        .contains("Showing Claude Desktop usage"));
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
            ("desktop:sd", LimitWindowKind::Weekly, 63.0, None),
            ("desktop:fh", LimitWindowKind::Session, 17.0, None),
        ]
    );
    assert_eq!(dtos[0].fetched_at, "2026-08-18T03:07:53.402Z");
}

#[test]
fn newer_on_n_off_usage_wins_without_a_desktop_source_note() {
    let rig = DesktopFallbackRig::new();
    rig.remember("2026-08-19T00:00:00.000Z", 39.0);

    let dtos = rig.read();

    assert_eq!(dtos[0].windows[0].used_percent, 39.0);
    assert_eq!(dtos[0].fetched_at, "2026-08-19T00:00:00.000Z");
    assert!(!dtos[0]
        .message
        .as_deref()
        .unwrap()
        .contains("Showing Claude Desktop usage"));
}

#[test]
fn freshness_compares_instants_instead_of_timestamp_text() {
    let rig = DesktopFallbackRig::new();
    rig.remember("2026-08-18T04:00:00.000+01:00", 39.0);

    let dtos = rig.read();

    assert_eq!(dtos[0].windows[0].id, "desktop:sd");
    assert!(dtos[0]
        .message
        .as_deref()
        .unwrap()
        .contains("Showing Claude Desktop usage"));
}
