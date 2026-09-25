//! A verified Claude user's remembered history, from the outside: shown as it was written when the
//! live read fails, and kept apart from the user-only history of the legacy key it replaces.

use crate::dto::{LimitWindowDto, LimitWindowKind};
use crate::http::{refused_url, serve_once};
use crate::limits::json::window;
use crate::limits::tests::refused_endpoints;
use crate::limits::*;
use crate::paths::scratch_dir;
use std::fs;
use std::path::PathBuf;

const EXPIRED_RENEWABLE_LOGIN: &str = r#"{"claudeAiOauth":{"accessToken":"kc-token","expiresAt":1787022473402,"refreshToken":"rt","refreshTokenExpiresAt":1787981634215,"subscriptionType":"max"}}"#;
/// The fixture login's `expiresAt`: a read just after it finds the access token expired.
const LOGIN_EXPIRES_AT_MS: i64 = 1787022473402;

struct ClaudeObservationRig {
    home: PathBuf,
    memo: ClaudeLoginMemo,
}

impl ClaudeObservationRig {
    fn new() -> Self {
        let home = scratch_dir("limits-claude-observation");
        write(
            &home.join(".claude").join(".credentials.json"),
            EXPIRED_RENEWABLE_LOGIN,
        );
        write(
            &home.join(".claude.json"),
            r#"{"oauthAccount":{"accountUuid":"uuid-1","emailAddress":"me@example.com","organizationUuid":"org-1"}}"#,
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
            .save(
                &ProviderLimitsDto::for_test(AgentId::Claude, id)
                    .labelled("me@example.com")
                    .with_reading(Reading {
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
                        ..Reading::default()
                    }),
            )
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
                keychain: |_| Ok(None),
                claude: refused_endpoints(&refused),
                now_ms: LOGIN_EXPIRES_AT_MS + 1,
            },
        )
    }
}

fn write(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn a_failed_read_shows_the_verified_users_remembered_history() {
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
    assert_eq!(dtos[0].reading.plan.as_deref(), Some("pro"));
    assert_eq!(
        dtos[0]
            .reading
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
        .reading
        .windows
        .iter()
        .all(|window| window.observed_at == "2026-08-17T15:00:00.000Z"));
}

#[test]
fn a_remembered_offset_timestamp_is_shown_as_it_was_written() {
    let rig = ClaudeObservationRig::new();
    rig.remember("2026-08-18T04:00:00.000+01:00", 39.0);

    let dtos = rig.read();

    let weekly = dtos[0]
        .reading
        .windows
        .iter()
        .find(|window| window.id == "weekly_all")
        .unwrap();
    assert_eq!(weekly.used_percent, 39.0);
    assert_eq!(weekly.observed_at, "2026-08-18T04:00:00.000+01:00");
}

#[test]
fn legacy_user_only_snapshot_is_retained_without_assigning_it_to_a_workspace() {
    let rig = ClaudeObservationRig::new();
    rig.remember_as("uuid-1", "2026-08-19T00:00:00.000Z", 39.0);
    let rows = rig.read();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].account.as_ref().unwrap().id.starts_with("profile:"));
    assert!(rows[0].reading.windows.is_empty());
    assert_eq!(rows[1].account.as_ref().unwrap().id, "uuid-1");
    assert!(!rows[1].current_account);
    assert_eq!(rows[1].reading.windows[0].used_percent, 39.0);
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
            keychain: |_| Ok(None),
            claude: ClaudeEndpoints {
                token: &refused_url(),
                profile: &profile,
                usage: &usage,
            },
            now_ms: LOGIN_EXPIRES_AT_MS - 1000,
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
        assert_eq!(
            result.dto.reading.plan.as_deref(),
            Some(expected),
            "tier: {tier:?}"
        );
    }
}
