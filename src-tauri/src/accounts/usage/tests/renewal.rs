//! When a saved login renews before or after its read.
use super::*;

fn policy_profile(provider: AgentId, owned: bool, expired: bool) -> Profile {
    use base64::Engine;
    let mut p = profile();
    p.identity.provider = provider;
    let jwt = |value: serde_json::Value| {
        format!(
            "header.{}.signature",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&value).unwrap())
        )
    };
    p.login = Some(Login {
        auth: if provider == AgentId::Claude {
            json!({"claudeAiOauth":{"accessToken":"access","refreshToken":"refresh","expiresAt":if expired {1} else {9_000_000}}})
        } else {
            json!({"tokens":{"access_token":jwt(json!({"exp":if expired {1} else {9_000_000}})),"refresh_token":"refresh","account_id":"team",
                "id_token":jwt(json!({"https://api.openai.com/auth":{"chatgpt_user_id":"user","chatgpt_account_id":"team"}}))}})
        },
        account: json!({"accountUuid":"user","organizationUuid":"team"}),
    });
    p.usage_renewal_owned = owned;
    p
}

#[test]
fn automatic_renewal_policy_covers_both_providers_and_never_renews_shadows() {
    use std::cell::Cell;
    for provider in [AgentId::Claude, AgentId::Codex] {
        for owned in [false, true] {
            for expired in [false, true] {
                for unauthorized in [false, true] {
                    let p = policy_profile(provider, owned, expired);
                    let reads = Cell::new(0);
                    let renewals = Cell::new(0);
                    let result = fetch_with(
                        &p,
                        1_000_000,
                        &|_| {
                            reads.set(reads.get() + 1);
                            if unauthorized {
                                Err(HttpError::Unauthorized.into())
                            } else {
                                Ok(reading(&p))
                            }
                        },
                        &|| {
                            renewals.set(renewals.get() + 1);
                            Ok(p.login.clone().unwrap())
                        },
                    );
                    assert_eq!(
                        renewals.get(),
                        usize::from(owned && (expired || unauthorized))
                    );
                    assert_eq!(
                        reads.get(),
                        if owned && !expired && unauthorized {
                            2
                        } else {
                            1
                        }
                    );
                    assert_eq!(result.result.is_err(), unauthorized);
                }
            }
        }
    }
}

/// A private login this owns, for `user` in `team`, whose access token expires as `expiry` says.
fn owned_with(provider: AgentId, expiry: Option<i64>) -> Profile {
    use base64::Engine;
    let jwt = |value: serde_json::Value| {
        format!(
            "header.{}.signature",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&value).unwrap())
        )
    };
    let mut p = profile();
    p.identity.provider = provider;
    p.usage_renewal_owned = true;
    p.login = Some(if provider == AgentId::Claude {
        let mut auth = json!({"claudeAiOauth":{"accessToken":"access","refreshToken":"refresh"}});
        if let Some(at) = expiry {
            auth["claudeAiOauth"]["expiresAt"] = json!(at);
        }
        Login {
            auth,
            account: json!({"accountUuid":"user","organizationUuid":"team"}),
        }
    } else {
        let access = expiry.map_or_else(|| "not-a-jwt".to_string(), |exp| jwt(json!({"exp": exp})));
        Login {
            auth: json!({"tokens":{"access_token":access,"refresh_token":"refresh","account_id":"team",
                "id_token":jwt(json!({"https://api.openai.com/auth":{"chatgpt_user_id":"user","chatgpt_account_id":"team"}}))}}),
            account: serde_json::Value::Null,
        }
    });
    p
}

/// When a saved login is due to renew before it is read, at 1,000,000 ms: a Claude one once its
/// access token's `expiresAt` (ms) is reached, never without one; a Codex one within ten minutes
/// of its access token's `exp` (s), or when that cannot be read.
#[test]
fn a_saved_login_renews_before_its_read_once_its_provider_says_it_is_due() {
    use std::cell::Cell;
    for (provider, expiry, due) in [
        (AgentId::Claude, Some(1_000_000), true),
        (AgentId::Claude, Some(1_000_001), false),
        (AgentId::Claude, None, false),
        (AgentId::Codex, Some(1_599), true),
        (AgentId::Codex, Some(1_600), false),
        (AgentId::Codex, None, true),
    ] {
        let p = owned_with(provider, expiry);
        let renewals = Cell::new(0);
        let result = fetch_with(&p, 1_000_000, &|_| Ok(reading(&p)), &|| {
            renewals.set(renewals.get() + 1);
            Ok(p.login.clone().unwrap())
        });
        assert!(result.result.is_ok(), "{provider:?} {expiry:?}");
        assert_eq!(
            renewals.get(),
            usize::from(due),
            "{provider:?} expiring at {expiry:?}"
        );
    }
}

#[test]
fn owned_renewal_uses_the_rotated_credential_for_usage() {
    use std::cell::Cell;
    for provider in [AgentId::Claude, AgentId::Codex] {
        for expired in [false, true] {
            let p = policy_profile(provider, true, expired);
            let fingerprint = |login: &Login| {
                crate::accounts::view(provider, login)
                    .unwrap()
                    .fingerprint()
            };
            let old = fingerprint(p.login.as_ref().unwrap());
            let mut rotated = p.login.clone().unwrap();
            if provider == AgentId::Claude {
                rotated.auth["claudeAiOauth"]["accessToken"] = json!("rotated-access");
            } else {
                rotated.auth["tokens"]["access_token"] = json!("rotated-access");
            }
            let reads = Cell::new(0);
            let result = fetch_with(
                &p,
                1_000_000,
                &|login| {
                    reads.set(reads.get() + 1);
                    if fingerprint(login) == old {
                        Err(HttpError::Unauthorized.into())
                    } else {
                        assert_eq!(fingerprint(login), fingerprint(&rotated));
                        Ok(reading(&p))
                    }
                },
                &|| Ok(rotated.clone()),
            );
            assert!(result.result.is_ok());
            assert_eq!(fingerprint(&result.login.unwrap()), fingerprint(&rotated));
            assert_eq!(reads.get(), if expired { 1 } else { 2 });
        }
    }
}

/// Renewal cannot make a login sign in as another account, so a read that found one spends no
/// refresh token, whoever owns the login and whether or not it has expired.
#[test]
fn a_login_that_signs_in_as_another_account_is_never_renewed_for_it() {
    use std::cell::Cell;
    for provider in [AgentId::Claude, AgentId::Codex] {
        let p = policy_profile(provider, true, false);
        let renewals = Cell::new(0);
        let result = fetch_with(
            &p,
            1_000_000,
            &|_| Err(SavedReadError::OtherAccount),
            &|| {
                renewals.set(renewals.get() + 1);
                Ok(p.login.clone().unwrap())
            },
        );
        assert_eq!(result.result.err(), Some(SavedReadError::OtherAccount));
        assert_eq!(renewals.get(), 0, "{provider:?}");
    }
}
