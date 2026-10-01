//! When a saved login renews before or after its read.
use super::*;

/// A saved Codex login for `user` in `team`, `owned` for private renewal, whose access token
/// expires at `exp` (s), or cannot be read without one.
fn codex_login(owned: bool, exp: Option<i64>) -> Profile {
    use base64::Engine;
    let jwt = |value: serde_json::Value| {
        format!(
            "header.{}.signature",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&value).unwrap())
        )
    };
    let access = exp.map_or_else(|| "not-a-jwt".to_string(), |exp| jwt(json!({"exp": exp})));
    let mut p = profile();
    p.identity.provider = AgentId::Codex;
    p.usage_renewal_owned = owned;
    p.login = Some(Login {
        auth: json!({"tokens":{"access_token":access,"refresh_token":"refresh","account_id":"team",
            "id_token":jwt(json!({"https://api.openai.com/auth":{"chatgpt_user_id":"user","chatgpt_account_id":"team"}}))}}),
        account: serde_json::Value::Null,
    });
    p
}

/// An access token long expired at the tests' 1,000,000 ms, or one far from expiring.
fn expiry(expired: bool) -> Option<i64> {
    Some(if expired { 1 } else { 9_000_000 })
}

/// Only a login this owns renews: before its read once it expired, or after a refused read.
#[test]
fn automatic_renewal_renews_only_an_owned_login_and_never_a_shadow() {
    use std::cell::Cell;
    for owned in [false, true] {
        for expired in [false, true] {
            for unauthorized in [false, true] {
                let p = codex_login(owned, expiry(expired));
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

/// A saved Codex login is due to renew before it is read within ten minutes of its access token's
/// `exp` (s), or when that cannot be read: at 1,000,000 ms, 1,599 s is due and 1,600 s is not.
#[test]
fn a_saved_login_renews_before_its_read_once_its_access_token_nears_expiry() {
    use std::cell::Cell;
    for (exp, due) in [(Some(1_599), true), (Some(1_600), false), (None, true)] {
        let p = codex_login(true, exp);
        let renewals = Cell::new(0);
        let result = fetch_with(&p, 1_000_000, &|_| Ok(reading(&p)), &|| {
            renewals.set(renewals.get() + 1);
            Ok(p.login.clone().unwrap())
        });
        assert!(result.result.is_ok(), "{exp:?}");
        assert_eq!(renewals.get(), usize::from(due), "expiring at {exp:?}");
    }
}

#[test]
fn owned_renewal_uses_the_rotated_credential_for_usage() {
    use std::cell::Cell;
    for expired in [false, true] {
        let p = codex_login(true, expiry(expired));
        let fingerprint = |login: &Login| {
            crate::accounts::view(AgentId::Codex, login)
                .unwrap()
                .fingerprint()
        };
        let old = fingerprint(p.login.as_ref().unwrap());
        let mut rotated = p.login.clone().unwrap();
        rotated.auth["tokens"]["access_token"] = json!("rotated-access");
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

/// Renewal cannot make a login sign in as another account, so a read that found one is not renewed
/// and read again, as a refused one is: an unexpired login is read once and never renewed, whoever
/// owns it.
#[test]
fn a_login_that_signs_in_as_another_account_is_never_renewed_for_it() {
    use std::cell::Cell;
    for owned in [false, true] {
        let p = codex_login(owned, expiry(false));
        let reads = Cell::new(0);
        let renewals = Cell::new(0);
        let result = fetch_with(
            &p,
            1_000_000,
            &|_| {
                reads.set(reads.get() + 1);
                Err(SavedReadError::OtherAccount)
            },
            &|| {
                renewals.set(renewals.get() + 1);
                Ok(p.login.clone().unwrap())
            },
        );
        assert_eq!(result.result.err(), Some(SavedReadError::OtherAccount));
        assert_eq!((reads.get(), renewals.get()), (1, 0), "owned: {owned}");
    }
}
