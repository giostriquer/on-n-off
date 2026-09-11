//! The renewal that keeps the Limits screen alive between `claude` runs, from the outside: an
//! expired stored login goes in, a live read comes out, and the store is left holding what the
//! caller was handed. The mechanism's own edges live in `limits/claude_renew/tests.rs`.

use super::*;
use crate::http::{head_header, serve_once_capturing};

/// What `platform.claude.com/v1/oauth/token` answers a `refresh_token` grant. `expires_in` is
/// the eight hours Claude Code's access tokens actually live; the reply rotates the refresh
/// token and says nothing about the subscription, which the stored login already knows.
const CLAUDE_REFRESH_REPLY: &str = r#"{"access_token":"renewed-token","refresh_token":"rt2","expires_in":28800,"refresh_token_expires_in":604800,"scope":"user:profile user:inference"}"#;

/// The renewal Claude Code performs for itself, done here instead. An access token lives eight
/// hours and only a `claude` run mints a new one, so any longer gap left a signed-in user staring
/// at "Access token expired" until they went and typed in a terminal.
#[test]
fn an_expired_claude_login_is_renewed_from_its_refresh_token_before_the_read() {
    let rig = Rig::new("limits-claude-renew");
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
    let after = 1787022473402 + 1;
    let (token_url, token_request) = serve_once_capturing("200 OK", &[], CLAUDE_REFRESH_REPLY);
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);

    let dtos = read_limits_in(
        AgentId::Claude,
        false,
        Sources {
            home: &rig.home,
            memo: &rig.memo,
            keychain: || Ok(None),
            claude: ClaudeEndpoints {
                token: &token_url,
                profile: &profile_url,
                usage: &usage_url,
            },
            claude_desktop_history: claude_desktop::history_path_for_home(&rig.home),
            now_ms: after,
        },
    );

    let token = token_request.join().unwrap();
    let grant: serde_json::Value = serde_json::from_str(&token.body).unwrap();
    assert_eq!(grant["grant_type"], "refresh_token");
    assert_eq!(
        grant["refresh_token"], "rt",
        "the stored refresh token is what gets redeemed"
    );
    assert!(
        !token.head.to_lowercase().contains("authorization:"),
        "the token endpoint authenticates by grant, not by the token being replaced: {}",
        token.head
    );
    profile_request.join().unwrap();
    let usage_head = usage_request.join().unwrap();
    assert_eq!(
        head_header(&usage_head, "authorization"),
        Some("Bearer renewed-token"),
        "{usage_head}"
    );
    assert_eq!(dtos[0].status, LimitsStatus::Ok, "{:?}", dtos[0].message);

    let stored: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(rig.home.join(".claude/.credentials.json")).unwrap(),
    )
    .unwrap();
    let oauth = &stored["claudeAiOauth"];
    assert_eq!(oauth["accessToken"], "renewed-token");
    assert_eq!(
        oauth["refreshToken"], "rt2",
        "a rotated refresh token is kept"
    );
    assert_eq!(oauth["expiresAt"].as_i64(), Some(after + 28_800_000));
    assert_eq!(
        oauth["subscriptionType"], "max",
        "fields the reply does not carry survive the write"
    );
}
