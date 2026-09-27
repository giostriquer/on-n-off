mod banked_resets;
mod login;
mod observation;
mod parse;
mod renewal;
mod saved;
mod subscription;

use super::*;
use crate::http::{head_header, refused_url, serve_once, serve_sequence};
use crate::limits::credentials::{self, read_claude_credential, ClaudeLoginMemo};
use crate::limits::snapshots::SnapshotStore;
use crate::limits::{read_limits_in, remember, CodexEndpoints};
use crate::paths::scratch_dir;
use serde_json::json;
use std::cell::Cell;
use std::fs;
use std::path::Path;

fn account(id: &str, label: &str) -> LimitsAccountDto {
    LimitsAccountDto {
        legacy_id: None,
        id: id.to_string(),
        label: Some(label.to_string()),
    }
}

fn write(home: &Path, rel: &str, body: &str) {
    let path = home.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

const CLAUDE_CREDENTIALS: &str = r#"{"claudeAiOauth":{"accessToken":"kc-token","expiresAt":1787022473402,"subscriptionType":"max"}}"#;
/// The same login as `CLAUDE_CREDENTIALS` with the refresh token Claude Code actually stores:
/// the access token lasts 8 hours, the refresh token more than a week.
const CLAUDE_RENEWABLE_CREDENTIALS: &str = r#"{"claudeAiOauth":{"accessToken":"kc-token","expiresAt":1787022473402,"refreshToken":"rt","refreshTokenExpiresAt":1787981634215,"subscriptionType":"max"}}"#;
/// Well before the Claude fixture's `expiresAt`.
const NOW_MS: i64 = 1787000000000;

/// Three endpoints that all refuse connections, for a read that must not reach the network at
/// all. One refused port answers for every one of them.
fn refused_endpoints(url: &str) -> ClaudeEndpoints<'_> {
    ClaudeEndpoints {
        token: url,
        profile: url,
        usage: url,
    }
}

/// Test doubles for `read_limits_in`: a scratch home, a fresh memo, a counting Keychain probe.
struct Rig {
    home: std::path::PathBuf,
    memo: ClaudeLoginMemo,
    probes: Cell<u32>,
    /// Refuses connections unless a renewal test replaces it: a read that reaches the token
    /// endpoint when it had a good login is a bug these tests should fail on.
    token_url: String,
    /// The whole probe outcome, not just its payload, so a denied or unanswered
    /// Keychain prompt is reachable from a test rather than only in the wild.
    keychain: KeychainProbe,
}

impl Rig {
    fn new(prefix: &str) -> Self {
        Self {
            home: scratch_dir(prefix),
            memo: ClaudeLoginMemo::new(),
            probes: Cell::new(0),
            token_url: refused_url(),
            keychain: Ok(None),
        }
    }

    /// The token endpoint stays refused: these tests hand out good logins, and one of them
    /// reaching for a renewal is a bug they should fail on.
    fn read(
        &self,
        agent: AgentId,
        force: bool,
        profile: &str,
        usage: &str,
    ) -> Vec<ProviderLimitsDto> {
        read_limits_in(
            agent,
            force,
            Sources {
                home: &self.home,
                now_ms: NOW_MS,
                claude: ClaudeSources {
                    memo: &self.memo,
                    keychain: |_| {
                        self.probes.set(self.probes.get() + 1);
                        self.keychain.clone()
                    },
                    endpoints: ClaudeEndpoints {
                        token: &self.token_url,
                        profile,
                        usage,
                    },
                },
            },
        )
    }
}

const CLAUDE_PAYLOAD: &str = r#"{"limits":[
        {"kind":"session","group":"session","percent":7,"resets_at":"2026-08-18T04:59:59+00:00"},
        {"kind":"weekly_all","group":"weekly","percent":12,"resets_at":"2026-08-24T13:59:59+00:00"}
    ]}"#;

const CLAUDE_PROFILE: &str =
    r#"{"account":{"uuid":"uuid-1","email":"me@example.com"},"organization":{"uuid":"org-1"}}"#;

fn claude_account_file(id: &str, email: &str) -> String {
    format!(
        r#"{{"oauthAccount":{{"accountUuid":"{id}","emailAddress":"{email}","organizationUuid":"org-1"}}}}"#
    )
}

fn claude_profile(id: &str, email: &str) -> String {
    format!(
        r#"{{"account":{{"uuid":"{id}","email":"{email}"}},"organization":{{"uuid":"org-1"}}}}"#
    )
}

fn account_of(dto: &ProviderLimitsDto) -> LimitsAccountDto {
    dto.account.clone().expect("account")
}

/// The one header set every Claude request sends: the token, the OAuth beta header, and no cached
/// answer. A GET states no content type.
fn assert_claude_headers(head: &str, authorization: &str) {
    assert_eq!(
        head_header(head, "authorization"),
        Some(authorization),
        "{head}"
    );
    assert_eq!(
        head_header(head, "anthropic-beta"),
        Some("oauth-2025-04-20"),
        "{head}"
    );
    assert_eq!(
        head_header(head, "cache-control"),
        Some("no-cache"),
        "{head}"
    );
    assert_eq!(head_header(head, "content-type"), None, "{head}");
}

#[test]
fn claude_pipeline_sends_the_oauth_headers_and_maps_the_payload() {
    let home = scratch_dir("limits-claude");
    write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    let lookup = read_claude_credential(&StorageDir::default_in(&home), Ok(None), NOW_MS);
    let dto = claude_limits(lookup, &None, &profile_url, &usage_url).dto;
    let profile_head = profile_request.join().unwrap();
    let usage_head = usage_request.join().unwrap();
    for head in [&profile_head, &usage_head] {
        assert_claude_headers(head, "Bearer kc-token");
    }
    assert_eq!(dto.status, LimitsStatus::Ok, "{:?}", dto.message);
    assert_eq!(dto.reading.plan.as_deref(), Some("max"));
    assert_eq!(dto.account, Some(account("uuid-1", "me@example.com")));
    assert!(dto.current_account);
    let ids: Vec<&str> = dto.reading.windows.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["weekly_all", "session"]);
}

#[test]
fn claude_rejects_usage_when_the_authenticated_profile_is_a_different_account() {
    let home = scratch_dir("limits-claude-mismatch");
    write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (url, request) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"uuid-other","email":"other@example.com"},"organization":{"uuid":"org-other"}}"#,
    );
    let lookup = read_claude_credential(&StorageDir::default_in(&home), Ok(None), NOW_MS);
    let dto = claude_limits(
        lookup,
        &Some(ClaudeIdentity {
            account: account("uuid-1", "me@example.com"),
            organization_id: Some("org-1".to_string()),
        }),
        &url,
        &refused_url(),
    )
    .dto;
    request.join().unwrap();

    assert_eq!(dto.status, LimitsStatus::Failed);
    assert_eq!(dto.account, Some(account("uuid-1", "me@example.com")));
    assert!(
        dto.message
            .as_deref()
            .unwrap()
            .contains("different account"),
        "{:?}",
        dto.message
    );
    assert!(dto.reading.windows.is_empty());
}

#[test]
fn claude_rejects_usage_when_the_authenticated_organization_is_different() {
    let home = scratch_dir("limits-claude-org-mismatch");
    write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (profile_url, profile_request) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"uuid-1","email":"me@example.com"},"organization":{"uuid":"org-other"}}"#,
    );
    let lookup = read_claude_credential(&StorageDir::default_in(&home), Ok(None), NOW_MS);
    let dto = claude_limits(
        lookup,
        &Some(ClaudeIdentity {
            account: account("uuid-1", "me@example.com"),
            organization_id: Some("org-1".to_string()),
        }),
        &profile_url,
        &refused_url(),
    )
    .dto;
    profile_request.join().unwrap();

    assert_eq!(dto.status, LimitsStatus::Failed);
    assert_eq!(dto.account, Some(account("uuid-1", "me@example.com")));
    assert!(dto.reading.windows.is_empty());
}

#[test]
fn a_throttled_claude_read_says_it_is_rate_limited() {
    let home = scratch_dir("limits-claude-throttled");
    write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (profile_url, profile_request) =
        crate::http::serve_once_capturing("429 Too Many Requests", &["Retry-After: 30"], "{}");
    let lookup = read_claude_credential(&StorageDir::default_in(&home), Ok(None), NOW_MS);
    let dto = claude_limits(lookup, &None, &profile_url, &refused_url()).dto;
    profile_request.join().unwrap();

    assert_eq!(dto.status, LimitsStatus::Failed);
    assert_eq!(
        dto.message.as_deref(),
        Some("Could not reach the Claude usage service (rate limited).")
    );
}

/// Claude Code's access token lasts 8 hours and the CLI renews it from its refresh token on
/// its next run. An expired access token is therefore not an expired login: the card must ask
/// for a `claude` run, never for a new sign-in, and must still name the signed-in account.
#[test]
fn an_expired_access_token_with_a_live_refresh_token_asks_only_for_a_cli_run() {
    let rig = Rig::new("limits-claude");
    write(
        &rig.home,
        ".claude/.credentials.json",
        CLAUDE_RENEWABLE_CREDENTIALS,
    );
    write(
        &rig.home,
        ".claude.json",
        &claude_account_file("uuid-1", "me@example.com"),
    );
    let refused = refused_url();
    let dtos = read_limits_in(
        AgentId::Claude,
        false,
        Sources {
            home: &rig.home,
            now_ms: 1787022473402 + 1,
            claude: ClaudeSources {
                memo: &rig.memo,
                keychain: |_| Ok(None),
                endpoints: refused_endpoints(&refused),
            },
        },
    );
    assert_eq!(dtos[0].status, LimitsStatus::Unauthenticated);
    let message = dtos[0].message.as_deref().unwrap();
    assert!(message.contains("`claude`"), "{message}");
    assert!(message.contains("send a prompt"), "{message}");
    assert!(!message.contains("any"), "{message}");
    assert!(!message.contains("sign in"), "{message}");
    assert_eq!(
        account_of(&dtos[0]).label.as_deref(),
        Some("me@example.com")
    );
}

/// A login whose refresh token has expired too really does need a new sign-in.
#[test]
fn an_expired_access_token_without_a_usable_refresh_token_asks_for_a_new_sign_in() {
    let rig = Rig::new("limits-claude");
    write(
        &rig.home,
        ".claude/.credentials.json",
        &CLAUDE_RENEWABLE_CREDENTIALS.replace("1787981634215", "1787022473402"),
    );
    let refused = refused_url();
    let dtos = read_limits_in(
        AgentId::Claude,
        false,
        Sources {
            home: &rig.home,
            now_ms: 1787022473402 + 1,
            claude: ClaudeSources {
                memo: &rig.memo,
                keychain: |_| Ok(None),
                endpoints: refused_endpoints(&refused),
            },
        },
    );
    assert_eq!(dtos[0].status, LimitsStatus::Unauthenticated);
    assert!(dtos[0]
        .message
        .as_deref()
        .unwrap()
        .contains("sign in again"));
}
