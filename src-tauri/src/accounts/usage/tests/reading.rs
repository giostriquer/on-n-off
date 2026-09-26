//! A saved profile's fetch through its provider's adapter (`Adapter::read_usage`).
use super::*;

/// A saved `provider` profile of `user` in `team` whose login holds `auth`, which on-n-off does not
/// own.
fn saved(provider: AgentId, auth: serde_json::Value) -> Profile {
    let mut p = profile();
    p.identity.provider = provider;
    p.login = Some(Login {
        auth,
        account: json!({"accountUuid":"user","organizationUuid":"team"}),
    });
    p
}

/// Services that refuse every connection, for a read that must not reach one, or that shows it did
/// by the network error it gets.
fn refused(url: &str) -> SavedReadUrls<'_> {
    SavedReadUrls {
        claude_profile: url,
        claude_usage: url,
        codex: crate::limits::CodexEndpoints {
            usage: url,
            reset_credits: url,
            credit_usage: url,
            subscriptions: url,
        },
    }
}

/// A login without an access token is refused by its adapter before any request, for either
/// provider.
#[test]
fn a_saved_login_without_an_access_token_is_refused_before_any_request() {
    for (provider, auth) in [
        (
            AgentId::Claude,
            json!({"claudeAiOauth":{"refreshToken":"fixture-refresh"}}),
        ),
        (
            AgentId::Codex,
            json!({"tokens":{"refresh_token":"fixture-refresh"}}),
        ),
    ] {
        let url = crate::http::refused_url();
        let p = saved(provider, auth);
        let result = crate::accounts::adapter(provider).unwrap().read_usage(
            &p.identity,
            p.login.as_ref().unwrap(),
            1_000_000,
            &refused(&url),
        );
        assert_eq!(
            result.err(),
            Some(HttpError::Unauthorized.into()),
            "{provider:?}"
        );
    }
}

/// When these fetches run, in ms.
const NOW: i64 = 1_000_000;

/// `p`'s fetch at [`NOW`], asking `urls`. A login on-n-off does not own is never renewed, so the
/// vault is never opened for one.
fn fetch(p: &Profile, urls: &SavedReadUrls<'_>) -> FetchResult {
    fetch_profile_at(
        p,
        &|| panic!("a login on-n-off does not own is never renewed"),
        NOW,
        urls,
    )
}

/// A Claude login on-n-off does not own whose access token has reached its `expiresAt` is not sent:
/// the fetch says it expired, where a request would have met the refused service.
#[test]
fn a_saved_claude_login_past_its_expiry_is_not_sent() {
    let url = crate::http::refused_url();
    for expires_at in [1, NOW] {
        let p = saved(
            AgentId::Claude,
            json!({"claudeAiOauth":{"accessToken":"access","refreshToken":"refresh","expiresAt":expires_at}}),
        );
        let fetched = fetch(&p, &refused(&url));
        assert_eq!(
            fetched.result.err(),
            Some(SavedReadError::Expired),
            "{expires_at}"
        );
    }
}

/// The same login a millisecond short of its expiry is sent, and meets the refused service.
#[test]
fn a_saved_claude_login_before_its_expiry_is_sent() {
    let url = crate::http::refused_url();
    let p = saved(
        AgentId::Claude,
        json!({"claudeAiOauth":{"accessToken":"access","refreshToken":"refresh","expiresAt":NOW + 1}}),
    );
    let fetched = fetch(&p, &refused(&url));
    assert!(
        matches!(
            fetched.result,
            Err(SavedReadError::Http(HttpError::Network(_)))
        ),
        "{:?}",
        fetched.result.err()
    );
}

/// A Codex login is read from the usage body with its own token, for its workspace, as the
/// profile's card.
#[test]
fn a_saved_codex_login_is_read_from_the_usage_body_with_its_own_token() {
    let (usage, request) = crate::http::serve_once_capturing(
        "200 OK",
        &[],
        r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":42,"limit_window_seconds":604800}}}"#,
    );
    let url = crate::http::refused_url();
    let mut p = saved(
        AgentId::Codex,
        json!({"tokens":{"access_token":"codex-access","refresh_token":"codex-refresh","account_id":"team",
        "id_token":crate::accounts::codex::tests::id_token(
            &json!({"chatgpt_user_id":"user","chatgpt_account_id":"team"})
        )}}),
    );
    p.login.as_mut().unwrap().account = serde_json::Value::Null;
    let urls = SavedReadUrls {
        codex: crate::limits::CodexEndpoints {
            usage: &usage,
            ..refused(&url).codex
        },
        ..refused(&url)
    };

    let card = fetch(&p, &urls).result.expect("the profile's card");

    let head = request.join().unwrap().head;
    assert_eq!(
        crate::http::head_header(&head, "authorization"),
        Some("Bearer codex-access"),
        "{head}"
    );
    assert_eq!(
        crate::http::head_header(&head, "chatgpt-account-id"),
        Some("team"),
        "{head}"
    );
    assert_eq!(card.account.unwrap().id, p.identity.observation_key());
    assert_eq!(card.reading.windows[0].used_percent, 42.0);
}
