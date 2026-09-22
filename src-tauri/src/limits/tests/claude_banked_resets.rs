//! Claude's saved rate-limit resets, from the outside: the live read asks the usage endpoint for
//! them and the card carries the count. The block's own edges live in `limits/claude/tests.rs`.

use super::*;

const USAGE_WITH_SAVED_RESETS: &str = r#"{
    "limits": [{"kind":"session","group":"session","percent":100,"resets_at":"2026-08-18T04:59:59+00:00"}],
    "cedar_ember": {
        "eligible": true,
        "at_limit": true,
        "grants": [{"id":"launch","label":"Saved reset","resets_total":1,"resets_left":1,
                    "ends_at":"2026-10-05T00:00:00Z","clears":["five_hour","seven_day"]}],
        "next_grant_id": "launch"
    }
}"#;

#[test]
fn the_live_claude_read_asks_for_saved_resets_and_the_card_carries_their_count() {
    let home = scratch_dir("limits-claude-saved-resets");
    write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", USAGE_WITH_SAVED_RESETS);
    let lookup = read_claude_credential(&home, Ok(None), NOW_MS);

    let dto = claude_limits(lookup, &None, &profile_url, &usage_url).dto;

    profile_request.join().unwrap();
    let usage_head = usage_request.join().unwrap();
    let request_line = usage_head.lines().next().unwrap_or_default();
    assert!(
        request_line.contains("?cedar_ember=1&skip_spend=1 "),
        "the saved-reset block only comes back when asked for: {request_line}"
    );
    assert_eq!(dto.status, LimitsStatus::Ok, "{:?}", dto.message);
    assert_eq!(
        dto.windows.len(),
        1,
        "the windows still come from the same read"
    );
    assert_eq!(
        dto.reset_credits,
        Some(LimitsResetCreditsDto {
            available_count: 1,
            next_expires_at: Some("2026-10-05T00:00:00+00:00".to_string()),
        })
    );
}
