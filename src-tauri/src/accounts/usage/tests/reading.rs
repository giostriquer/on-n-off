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
        let p = saved(provider, auth);
        let result = crate::accounts::adapter(provider).unwrap().read_usage(
            &p.identity,
            p.login.as_ref().unwrap(),
            1_000_000,
        );
        assert_eq!(
            result.err(),
            Some(HttpError::Unauthorized.into()),
            "{provider:?}"
        );
    }
}
