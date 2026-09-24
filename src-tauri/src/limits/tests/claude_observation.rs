use crate::dto::LimitWindowKind;
use crate::http::{refused_url, serve_once};
use crate::limits::json::window;
use crate::limits::tests::refused_endpoints;
use crate::limits::*;
use crate::paths::scratch_dir;
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
        self.remember_as(
            "profile:52f46058343b7e483573e358fb619b64fb816f9a8dbcf50006bd9d687755ec58",
            observed_at,
            used_percent,
        );
    }
    fn remember_as(&self, id: &str, observed_at: &str, used_percent: f64) {
        SnapshotStore::for_home(&self.home)
            .save(&ProviderLimitsDto {
                provider: AgentId::Claude,
                status: LimitsStatus::Ok,
                message: None,
                account: Some(LimitsAccountDto {
                    legacy_id: None,
                    id: id.to_string(),
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
                workspace_credits: None,
                reset_credits: None,
                reset_offer: None,
            })
            .unwrap();
    }

    fn read(&self) -> Vec<ProviderLimitsDto> {
        let refused = refused_url();
        read_limits_in(
            AgentId::Claude,
            false,
            Sources {
                home: &self.home,
                memo: &self.memo,
                keychain: || Ok(None),
                claude: refused_endpoints(&refused),
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
fn organization_only_observations_do_not_replace_verified_user_history() {
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
        [("weekly_all", LimitWindowKind::Weekly, 39.0, None)]
    );
    assert!(dtos[0]
        .windows
        .iter()
        .all(|window| window.observed_at == "2026-08-17T15:00:00.000Z"));
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
    assert!(dtos[0].windows.iter().all(|window| window.id != "session"));
    assert_eq!(weekly.used_percent, 39.0);
    assert_eq!(weekly.observed_at, "2026-08-19T00:00:00.000Z");
    assert!(!dtos[0]
        .message
        .as_deref()
        .unwrap()
        .contains("Showing Claude Desktop usage"));
}

#[test]
fn verified_offset_timestamp_is_preserved_without_unattributed_samples() {
    let rig = ClaudeObservationRig::new();
    rig.remember("2026-08-18T04:00:00.000+01:00", 39.0);

    let dtos = rig.read();

    let weekly = dtos[0]
        .windows
        .iter()
        .find(|window| window.id == "weekly_all")
        .unwrap();
    assert_eq!(weekly.used_percent, 39.0);
    assert_eq!(weekly.observed_at, "2026-08-18T04:00:00.000+01:00");
    assert!(!dtos[0].message.as_deref().unwrap().contains("Desktop"));
}

#[test]
fn a_successful_endpoint_read_remains_authoritative_over_local_windows() {
    let rig = ClaudeObservationRig::new();
    write(
        &rig.home.join(".claude").join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"token","expiresAt":1787023473402,"refreshToken":"rt","refreshTokenExpiresAt":1787981634215,"subscriptionType":"max"}}"#,
    );
    let (profile_url, profile_request) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"uuid-1","email":"me@example.com"},"organization":{"uuid":"org-1"}}"#,
    );
    let (usage_url, usage_request) = serve_once(
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
            claude: ClaudeEndpoints {
                token: &refused_url(),
                profile: &profile_url,
                usage: &usage_url,
            },
            claude_desktop_history: claude_desktop::history_path_for_home(&rig.home),
            now_ms: DESKTOP_TIMESTAMP_MS + 1,
        },
    );
    profile_request.join().unwrap();
    usage_request.join().unwrap();

    assert_eq!(dtos[0].status, LimitsStatus::Ok);
    assert_eq!(dtos[0].windows.len(), 1);
    assert_eq!(dtos[0].windows[0].id, "weekly_all");
    assert_eq!(dtos[0].windows[0].used_percent, 50.0);
    let persisted = SnapshotStore::for_home(&rig.home).load(AgentId::Claude);
    assert_eq!(persisted[0].windows.len(), 1);
    assert_eq!(persisted[0].windows[0].used_percent, 50.0);
}

#[test]
fn legacy_user_only_snapshot_is_retained_without_assigning_it_to_a_workspace() {
    let rig = ClaudeObservationRig::new();
    rig.remember_as("uuid-1", "2026-08-19T00:00:00.000Z", 39.0);
    let rows = rig.read();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].account.as_ref().unwrap().id.starts_with("profile:"));
    assert!(rows[0].windows.is_empty());
    assert_eq!(rows[1].account.as_ref().unwrap().id, "uuid-1");
    assert!(!rows[1].current_account);
    assert_eq!(rows[1].windows[0].used_percent, 39.0);
}

#[test]
fn organization_only_desktop_usage_is_never_assigned_to_either_user() {
    let rig = ClaudeObservationRig::new();
    for user in ["user-a", "user-b"] {
        write(
            &rig.home.join(".claude.json"),
            &format!(r#"{{"oauthAccount":{{"accountUuid":"{user}","organizationUuid":"org-1"}}}}"#),
        );
        let rows = rig.read();
        assert!(
            rows.iter().all(|row| row.windows.is_empty()),
            "organization-only sample became a user's usage"
        );
    }
}

#[test]
fn verified_claude_read_supersedes_its_legacy_user_card() {
    let rig = ClaudeObservationRig::new();
    rig.remember_as("uuid-1", "2026-08-17T15:00:00.000Z", 90.0);
    let (profile, profile_request) = serve_once("200 OK", super::CLAUDE_PROFILE);
    let (usage, usage_request) = serve_once("200 OK", super::CLAUDE_PAYLOAD);
    let dtos = read_limits_in(
        AgentId::Claude,
        false,
        Sources {
            home: &rig.home,
            memo: &rig.memo,
            keychain: || Ok(None),
            claude: ClaudeEndpoints {
                token: &refused_url(),
                profile: &profile,
                usage: &usage,
            },
            claude_desktop_history: claude_desktop::history_path_for_home(&rig.home),
            now_ms: DESKTOP_TIMESTAMP_MS - 1000,
        },
    );
    profile_request.join().unwrap();
    usage_request.join().unwrap();
    assert_eq!(dtos[0].status, LimitsStatus::Ok, "{:?}", dtos[0].message);
    assert_eq!(dtos.len(), 1);
    assert_eq!(
        dtos[0].account.as_ref().unwrap().legacy_id.as_deref(),
        Some("uuid-1")
    );
    assert_eq!(
        SnapshotStore::for_home(&rig.home)
            .load(AgentId::Claude)
            .len(),
        1
    );
}

#[test]
fn claude_max_plan_uses_known_login_tiers_and_keeps_unknown_tiers_generic() {
    for (subscription, tier, expected) in [
        ("max", Some("default_claude_max_5x"), "max ×5"),
        ("max", Some("default_claude_max_20x"), "max ×20"),
        ("max", None, "max"),
        ("max", Some("default_claude_max_50x"), "max"),
        ("pro", Some("default_claude_max_20x"), "pro"),
    ] {
        let credential = credentials::parse_claude_credential(&serde_json::json!({
            "claudeAiOauth": {"accessToken": "fixture-token", "subscriptionType": subscription, "rateLimitTier": tier}
        })).unwrap();
        let (profile, profile_request) = serve_once("200 OK", super::CLAUDE_PROFILE);
        let (usage, usage_request) = serve_once("200 OK", super::CLAUDE_PAYLOAD);
        let result = claude_limits(CredentialLookup::Found(credential), &None, &profile, &usage);
        profile_request.join().unwrap();
        usage_request.join().unwrap();
        assert_eq!(result.dto.plan.as_deref(), Some(expected), "tier: {tier:?}");
    }
}
