use super::*;

fn saved(provider: AgentId, auth: serde_json::Value) -> Profile {
    let mut p = profile();
    p.identity.provider = provider;
    p.login = Some(Login {
        auth,
        account: json!({"accountUuid":"user","organizationUuid":"team"}),
    });
    p
}

fn refused(url: &str) -> SavedReadUrls<'_> {
    SavedReadUrls {
        codex: crate::limits::CodexEndpoints {
            usage: url,
            reset_credits: url,
            credit_usage: url,
            subscriptions: url,
        },
    }
}

#[test]
fn a_saved_login_without_an_access_token_is_refused_before_any_request() {
    let url = crate::http::refused_url();
    let p = saved(
        AgentId::Codex,
        json!({"tokens":{"refresh_token":"fixture-refresh"}}),
    );
    let result = crate::accounts::adapter(AgentId::Codex)
        .unwrap()
        .read_usage(
            &p.identity,
            p.login.as_ref().unwrap(),
            1_000_000,
            &refused(&url),
        );
    assert_eq!(result.err(), Some(HttpError::Unauthorized.into()));
}

const NOW: i64 = 1_000_000;

fn fetch(p: &Profile, urls: &SavedReadUrls<'_>) -> FetchResult {
    fetch_profile_at(
        p,
        &|| panic!("a login on-n-off does not own is never renewed"),
        NOW,
        urls,
    )
}

#[test]
fn a_saved_claude_login_in_the_vault_is_never_sent() {
    let url = crate::http::refused_url();
    for expires_at in [1, NOW + 1] {
        let p = saved(
            AgentId::Claude,
            json!({"claudeAiOauth":{"accessToken":"access","refreshToken":"refresh","expiresAt":expires_at}}),
        );
        let fetched = fetch(&p, &refused(&url));
        assert_eq!(
            fetched.result.err(),
            Some(SavedReadError::Unavailable(
                "This account's login has not moved into its home yet; the next read tries again."
            )),
            "{expires_at}"
        );
    }
}

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
