//! Claude's saved rate-limit resets, from the outside: the live read asks the usage endpoint for
//! them, never lets that optional question cost the windows or the login, and a read that cannot
//! tell keeps the count the card already had. The block's own edges live in `limits/claude/tests.rs`.

use super::*;

/// Far enough ahead that the grant never reads as ended, whenever the suite runs.
const USE_BY: &str = "2099-10-05T00:00:00Z";

fn usage_with_saved_resets(block: &str) -> String {
    format!(
        r#"{{"limits":[{{"kind":"session","group":"session","percent":100,"resets_at":"2026-08-18T04:59:59+00:00"}}],"cedar_ember":{block}}}"#
    )
}

fn one_saved_reset() -> String {
    usage_with_saved_resets(&format!(
        r#"{{"eligible":true,"at_limit":true,"grants":[{{"id":"launch","resets_left":1,"ends_at":"{USE_BY}","clears":["five_hour","seven_day"]}}],"next_grant_id":"launch"}}"#
    ))
}

fn request_line(head: &str) -> &str {
    head.lines().next().unwrap_or_default()
}

#[test]
fn the_live_claude_read_asks_for_saved_resets_and_the_card_carries_their_count() {
    let home = scratch_dir("limits-claude-saved-resets");
    write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", &one_saved_reset());
    let lookup = read_claude_credential(&home, Ok(None), NOW_MS);

    let dto = claude_limits(lookup, &None, &profile_url, &usage_url).dto;

    profile_request.join().unwrap();
    let usage_head = usage_request.join().unwrap();
    assert!(
        request_line(&usage_head).contains("?cedar_ember=1&skip_spend=1 "),
        "the saved-reset block only comes back when asked for: {usage_head}"
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
            next_expires_at: Some("2099-10-05T00:00:00+00:00".to_string()),
        })
    );
}

/// The reset query is Claude Code's on-demand `/limit-reset` read, not the one it polls with. If
/// Anthropic ever refuses it to on-n-off, a 403 must not read as a rejected login: that would
/// clear the memo, tell a signed-in user to sign in again, and on the saved path spend a refresh
/// token, all for an optional row.
#[test]
fn a_refused_reset_query_falls_back_to_the_plain_read_instead_of_failing_the_login() {
    let home = scratch_dir("limits-claude-saved-resets-refused");
    write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_requests) = serve_sequence(&[
        ("403 Forbidden", &[], "{}"),
        ("200 OK", &[], CLAUDE_PAYLOAD),
    ]);
    let lookup = read_claude_credential(&home, Ok(None), NOW_MS);

    let dto = claude_limits(lookup, &None, &profile_url, &usage_url).dto;

    profile_request.join().unwrap();
    let requests = usage_requests.join().unwrap();
    assert!(request_line(&requests[0].head).contains("?cedar_ember=1&skip_spend=1 "));
    assert!(
        !request_line(&requests[1].head).contains('?'),
        "the fallback is the plain read: {}",
        requests[1].head
    );
    assert_eq!(
        head_header(&requests[1].head, "authorization"),
        Some("Bearer kc-token")
    );
    assert_eq!(dto.status, LimitsStatus::Ok, "{:?}", dto.message);
    assert_eq!(dto.windows.len(), 2);
    assert_eq!(
        dto.reset_credits, None,
        "a read that was not asked is unknown"
    );
}

/// Offline, the plain read would only wait on the same network, so a transport failure is final.
#[test]
fn a_network_failure_on_the_reset_query_is_not_retried() {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/usage", listener.local_addr().unwrap());
    let hang_up = std::thread::spawn(move || {
        drop(crate::http::accept_in_time(&listener));
        listener
    });

    let result = claude_usage(&url, &[]);

    let listener = hang_up.join().unwrap();
    assert!(matches!(result, Err(HttpError::Network(_))), "{result:?}");
    listener.set_nonblocking(true).unwrap();
    assert!(
        listener.accept().is_err(),
        "a second request went out after the connection dropped"
    );
}

#[test]
fn a_login_the_plain_read_also_rejects_is_still_reported_as_one() {
    let home = scratch_dir("limits-claude-saved-resets-rejected");
    write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_requests) = serve_sequence(&[
        ("401 Unauthorized", &[], "{}"),
        ("401 Unauthorized", &[], "{}"),
    ]);
    let lookup = read_claude_credential(&home, Ok(None), NOW_MS);

    let dto = claude_limits(lookup, &None, &profile_url, &usage_url).dto;

    profile_request.join().unwrap();
    assert_eq!(usage_requests.join().unwrap().len(), 2);
    assert_eq!(dto.status, LimitsStatus::Unauthenticated);
}

fn signed_in_rig(name: &str) -> Rig {
    let rig = Rig::new(name);
    write(&rig.home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    write(
        &rig.home,
        ".claude.json",
        &claude_account_file("uuid-1", "me@example.com"),
    );
    rig
}

/// One live read answering `usage`: the reset count on the signed-in card, and the one the next
/// read will load from its snapshot.
fn read_counts(rig: &Rig, usage: &str) -> (Option<u32>, Option<u32>) {
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", usage);
    let dtos = rig.read(AgentId::Claude, false, &profile_url, &usage_url);
    profile_request.join().unwrap();
    usage_request.join().unwrap();
    let card = dtos.into_iter().find(|dto| dto.current_account).unwrap();
    let stored = SnapshotStore::for_home(&rig.home)
        .load(AgentId::Claude)
        .into_iter()
        .find(|dto| dto.account == card.account)
        .unwrap();
    (
        card.reset_credits.map(|resets| resets.available_count),
        stored.reset_credits.map(|resets| resets.available_count),
    )
}

#[test]
fn a_read_that_cannot_tell_keeps_the_remembered_count_and_an_answer_of_none_replaces_it() {
    let rig = signed_in_rig("limits-claude-saved-resets-memory");
    let read = |usage: &str| read_counts(&rig, usage);

    assert_eq!(read(&one_saved_reset()), (Some(1), Some(1)));
    assert_eq!(
        read(&usage_with_saved_resets(
            r#"{"eligible":false,"ineligible_reason":"unavailable"}"#
        )),
        (Some(1), Some(1)),
        "an answer that could not say keeps what the card knew"
    );
    assert_eq!(
        read(CLAUDE_PAYLOAD),
        (Some(1), Some(1)),
        "so does a read without the block"
    );
    assert_eq!(
        read(&usage_with_saved_resets(r#"{"eligible":true,"grants":[]}"#)),
        (Some(0), Some(0)),
        "an answer of none is an answer"
    );
}

/// The reported case: a remembered count whose soonest expiry has passed, followed by reads that
/// cannot tell the count, which is every read Anthropic answers with `surface`.
#[test]
fn a_remembered_count_past_its_expiry_is_not_kept_by_a_read_that_cannot_tell() {
    let rig = signed_in_rig("limits-claude-saved-resets-lapsed");
    assert_eq!(read_counts(&rig, &one_saved_reset()), (Some(1), Some(1)));
    for entry in std::fs::read_dir(rig.home.join(".on-n-off/limits")).unwrap() {
        let path = entry.unwrap().path();
        let mut stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        if let Some(resets) = stored.get_mut("resetCredits") {
            resets["nextExpiresAt"] = "2020-01-01T00:00:00+00:00".into();
            std::fs::write(&path, stored.to_string()).unwrap();
        }
    }

    let surface =
        usage_with_saved_resets(r#"{"eligible":false,"ineligible_reason":"surface","grants":[]}"#);
    assert_eq!(read_counts(&rig, &surface), (None, None));
    assert_eq!(read_counts(&rig, &surface), (None, None));
}
