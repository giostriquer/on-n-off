use super::*;
use crate::accounts::{
    model,
    store::{ChangeKind, Database},
};
use serde_json::json;
fn profile() -> Profile {
    let mut db = super::super::store::Database::default();
    let identity = model::Identity {
        provider: AgentId::Claude,
        user_id: "user".into(),
        workspace_id: "team".into(),
    };
    db.save(
        identity,
        Login {
            auth: json!({}),
            account: json!({}),
        },
        None,
    )
    .unwrap();
    db.profiles.remove(0)
}
#[test]
fn a_failed_inactive_read_preserves_its_last_numbers_and_timestamp() {
    let profile = profile();
    let mut entries = vec![];
    merge(&mut entries, &profile, Some(Err("paused".into())));
    entries[0].reading.windows.push(crate::dto::LimitWindowDto {
        id: "weekly".into(),
        label: "Weekly".into(),
        kind: crate::dto::LimitWindowKind::Weekly,
        used_percent: 73.0,
        resets_at: None,
        observed_at: "2026-09-01T00:00:00Z".into(),
        window_seconds: Some(604800),
    });
    merge(&mut entries, &profile, Some(Err("unavailable".into())));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reading.windows[0].used_percent, 73.0);
    assert_eq!(
        entries[0].reading.windows[0].observed_at,
        "2026-09-01T00:00:00Z"
    );
    assert_eq!(entries[0].status, LimitsStatus::Failed);
    assert!(!entries[0].current_account);
}
#[test]
fn a_saved_read_that_cannot_tell_keeps_the_banked_reset_count_and_an_answer_replaces_it() {
    use crate::dto::LimitsResetCreditsDto;
    let profile = profile();
    let banked = |available_count| {
        Some(LimitsResetCreditsDto {
            available_count,
            next_expires_at: None,
        })
    };
    let read = |reset_credits| {
        let mut dto = reading(&profile);
        dto.reading.reset_credits = reset_credits;
        Some(Ok(dto))
    };
    let mut entries = vec![];

    merge(&mut entries, &profile, read(banked(1)));
    merge(&mut entries, &profile, read(None));
    assert_eq!(entries[0].reading.reset_credits, banked(1));

    merge(&mut entries, &profile, read(banked(0)));
    assert_eq!(entries[0].reading.reset_credits, banked(0));
}

#[test]
fn inactive_results_never_replace_the_active_account() {
    let profile = profile();
    let mut entries = vec![];
    merge(&mut entries, &profile, Some(Err("first".into())));
    entries[0].current_account = true;
    let before = entries.clone();
    merge(&mut entries, &profile, Some(Err("late saved read".into())));
    assert_eq!(entries, before);
}

fn stored(home: &Path) -> Profile {
    let mut p = profile();
    p.login = Some(Login {
        auth: json!({"claudeAiOauth":{"accessToken":"old","refreshToken":"refresh"}}),
        account: json!({"accountUuid":"user","organizationUuid":"team"}),
    });
    rewrite(home, |db| db.profiles.push(p.clone()));
    p
}
fn open(home: &Path) -> Store {
    Store::open_with_key(home, true, |_, _| Ok([8; 32])).unwrap()
}
/// A fixture write: no rule refuses it, and it rejects nothing in flight.
fn rewrite(home: &Path, edit: impl FnOnce(&mut Database)) {
    open(home)
        .change(ChangeKind::Metadata, |db| {
            edit(db);
            Ok(())
        })
        .unwrap();
}
/// An account change, which rejects every reading in flight.
fn account_change(home: &Path, edit: impl FnOnce(&mut Database)) {
    open(home)
        .change(ChangeKind::Account, |db| {
            edit(db);
            Ok(())
        })
        .unwrap();
}
/// The ticket a refresh starting now takes.
fn ticket(home: &Path) -> Ticket {
    open(home).load().unwrap().ticket(Guard::SignIn).unwrap()
}
fn reading(profile: &Profile) -> ProviderLimitsDto {
    let mut entries = vec![];
    merge(&mut entries, profile, Some(Err("fixture".into())));
    let mut dto = entries.remove(0);
    dto.status = LimitsStatus::Ok;
    dto.message = None;
    dto.reading.windows = vec![crate::dto::LimitWindowDto {
        id: "weekly".into(),
        label: "Weekly".into(),
        kind: crate::dto::LimitWindowKind::Weekly,
        used_percent: 42.0,
        resets_at: None,
        observed_at: "2026-09-19T00:00:00Z".into(),
        window_seconds: Some(604800),
    }];
    dto
}
#[test]
fn a_successful_saved_read_persists_numbers_without_changing_the_vault_login() {
    let home = tempfile::tempdir().unwrap();
    let p = stored(home.path());
    let before = std::fs::read(home.path().join(".on-n-off/accounts/vault.enc")).unwrap();
    let result = poll_with(
        home.path(),
        &p,
        &ticket(home.path()),
        false,
        &|| Ok(open(home.path())),
        &|p| FetchResult {
            login: p.login.clone(),
            result: Ok(reading(p)),
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(result.reading.windows[0].used_percent, 42.0);
    assert!(home
        .path()
        .join(".on-n-off/limits")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    assert_eq!(
        std::fs::read(home.path().join(".on-n-off/accounts/vault.enc")).unwrap(),
        before
    );
}
#[test]
fn removal_or_reauthentication_during_http_discards_the_late_read() {
    for remove in [true, false] {
        let home = tempfile::tempdir().unwrap();
        let p = stored(home.path());
        let result = poll_with(
            home.path(),
            &p,
            &ticket(home.path()),
            false,
            &|| Ok(open(home.path())),
            &|p| {
                account_change(home.path(), |db| {
                    if remove {
                        db.profiles.clear();
                    } else {
                        db.profiles[0].login.as_mut().unwrap().auth["claudeAiOauth"]
                            ["accessToken"] = json!("replacement");
                    }
                });
                FetchResult {
                    login: p.login.clone(),
                    result: Ok(reading(p)),
                }
            },
        );
        assert!(result.is_none());
        assert!(!home.path().join(".on-n-off/limits").exists());
    }
}
/// A remembered login or a private renewal can replace a saved login without an account change:
/// the login the reading was made with, not only the epoch, vouches for its publication.
#[test]
fn a_login_replaced_without_an_account_change_during_http_discards_the_late_read() {
    let home = tempfile::tempdir().unwrap();
    let p = stored(home.path());
    let result = poll_with(
        home.path(),
        &p,
        &ticket(home.path()),
        false,
        &|| Ok(open(home.path())),
        &|p| {
            rewrite(home.path(), |db| {
                db.profiles[0].login.as_mut().unwrap().auth["claudeAiOauth"]["accessToken"] =
                    json!("replacement");
            });
            FetchResult {
                login: p.login.clone(),
                result: Ok(reading(p)),
            }
        },
    );
    assert!(result.is_none());
    assert!(!home.path().join(".on-n-off/limits").exists());
}
/// Any account change while HTTP is in flight discards the reading, even one that left this
/// profile's login alone: the epoch, not only the login, vouches for a usage publication.
#[test]
fn an_unrelated_account_change_during_http_discards_the_late_read() {
    let home = tempfile::tempdir().unwrap();
    let p = stored(home.path());
    let result = poll_with(
        home.path(),
        &p,
        &ticket(home.path()),
        false,
        &|| Ok(open(home.path())),
        &|p| {
            account_change(home.path(), |_| {});
            FetchResult {
                login: p.login.clone(),
                result: Ok(reading(p)),
            }
        },
    );
    assert!(result.is_none());
    assert!(!home.path().join(".on-n-off/limits").exists());
}
#[test]
fn a_pending_recovery_during_http_discards_the_late_read() {
    let home = tempfile::tempdir().unwrap();
    let p = stored(home.path());
    let result = poll_with(
        home.path(),
        &p,
        &ticket(home.path()),
        false,
        &|| Ok(open(home.path())),
        &|p| {
            rewrite(home.path(), |db| {
                db.begin_recovery(super::super::transaction::Recovery {
                    target_id: p.id.clone(),
                    outgoing: None,
                    outgoing_identity: None,
                })
            });
            FetchResult {
                login: p.login.clone(),
                result: Ok(reading(p)),
            }
        },
    );
    assert!(result.is_none());
    assert!(!home.path().join(".on-n-off/limits").exists());
}
#[test]
fn forced_refresh_respects_rate_limit_backoff_per_account() {
    use std::cell::Cell;
    let home = tempfile::tempdir().unwrap();
    let p = stored(home.path());
    let calls = Cell::new(0);
    for force in [false, true, true] {
        let result = poll_with(
            home.path(),
            &p,
            &ticket(home.path()),
            force,
            &|| Ok(open(home.path())),
            &|_| {
                calls.set(calls.get() + 1);
                FetchResult {
                    login: p.login.clone(),
                    result: Err(HttpError::RateLimited(RateLimitReset::RetryAfter(1800))),
                }
            },
        );
        assert!(result.unwrap().is_err());
    }
    assert_eq!(calls.get(), 1);
    // A different saved account is not held behind this account's backoff.
    let mut other = p.clone();
    other.id = "other-profile".into();
    other.identity.user_id = "other".into();
    rewrite(home.path(), |db| db.profiles.push(other.clone()));
    assert!(poll_with(
        home.path(),
        &other,
        &ticket(home.path()),
        false,
        &|| Ok(open(home.path())),
        &|p| FetchResult {
            login: p.login.clone(),
            result: Ok(reading(p))
        }
    )
    .unwrap()
    .is_ok());
}
#[test]
fn a_new_credential_retries_a_previously_rejected_account() {
    let home = tempfile::tempdir().unwrap();
    let mut p = stored(home.path());
    assert!(poll_with(
        home.path(),
        &p,
        &ticket(home.path()),
        false,
        &|| Ok(open(home.path())),
        &|p| {
            FetchResult {
                login: p.login.clone(),
                result: Err(HttpError::Unauthorized),
            }
        }
    )
    .unwrap()
    .is_err());
    p.login.as_mut().unwrap().auth["claudeAiOauth"]["accessToken"] = json!("new-access");
    rewrite(home.path(), |db| db.profiles[0] = p.clone());
    assert!(poll_with(
        home.path(),
        &p,
        &ticket(home.path()),
        false,
        &|| Ok(open(home.path())),
        &|p| {
            FetchResult {
                login: p.login.clone(),
                result: Ok(reading(p)),
            }
        }
    )
    .unwrap()
    .is_ok());
}

#[test]
fn post_rotation_failures_keep_the_new_generation_backoff() {
    for rejected in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let p = stored(home.path());
        let result = poll_with(
            home.path(),
            &p,
            &ticket(home.path()),
            false,
            &|| Ok(open(home.path())),
            &|_| {
                let mut login = p.login.clone().unwrap();
                login.auth["claudeAiOauth"]["accessToken"] = json!("rotated");
                rewrite(home.path(), |db| db.profiles[0].login = Some(login.clone()));
                FetchResult {
                    login: Some(login),
                    result: Err(if rejected {
                        HttpError::Unauthorized
                    } else {
                        HttpError::RateLimited(RateLimitReset::RetryAfter(1800))
                    }),
                }
            },
        );
        assert!(result.expect("rotation error must remain visible").is_err());
        let p = open(home.path()).load().unwrap().profiles.remove(0);
        let result = poll_with(
            home.path(),
            &p,
            &ticket(home.path()),
            true,
            &|| Ok(open(home.path())),
            &|_| panic!("forced refresh must retain the renewed generation's backoff"),
        );
        assert!(result.unwrap().is_err());
    }
}

#[test]
fn native_api_key_login_does_not_block_saved_subscription_polling() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let home = tempfile::tempdir().unwrap();
    let mut p = stored(home.path());
    p.identity.provider = AgentId::Codex;
    rewrite(home.path(), |db| db.profiles[0] = p.clone());
    let calls = AtomicUsize::new(0);
    let mut entries = vec![];
    refresh_with(
        home.path(),
        AgentId::Codex,
        false,
        &mut entries,
        Ok(Some(Login {
            auth: json!({"OPENAI_API_KEY":"fixture-api-key"}),
            account: json!({}),
        })),
        &|| Ok(open(home.path())),
        &|p| {
            calls.fetch_add(1, Ordering::SeqCst);
            FetchResult {
                login: p.login.clone(),
                result: Ok(reading(p)),
            }
        },
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(entries[0].reading.windows[0].used_percent, 42.0);
}

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
                                Err(HttpError::Unauthorized)
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
fn shared_limits_reader_polls_inactive_accounts_once_and_preserves_active_results() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let home = tempfile::tempdir().unwrap();
    let inactive = stored(home.path());
    let mut active = inactive.clone();
    active.id = "active-profile".into();
    active.identity.user_id = "active-user".into();
    active.login.as_mut().unwrap().account["accountUuid"] = json!("active-user");
    rewrite(home.path(), |db| db.profiles.push(active.clone()));
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let native_calls = AtomicUsize::new(0);
    let mut current = reading(&active);
    current.current_account = true;
    current.reading.windows[0].used_percent = 19.0;
    let fetch_calls = calls.clone();
    TEST_REFRESH.with(|fixture| {
        *fixture.borrow_mut() = Some(Box::new(move |force, entries| {
            refresh_with(
                home.path(),
                AgentId::Claude,
                force,
                entries,
                Ok(active.login.clone()),
                &|| Ok(open(home.path())),
                &|p| {
                    assert_eq!(p.id, inactive.id);
                    fetch_calls.fetch_add(1, Ordering::SeqCst);
                    FetchResult {
                        login: p.login.clone(),
                        result: Ok(reading(p)),
                    }
                },
            );
        }))
    });
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_REFRESH.with(|f| *f.borrow_mut() = None);
        }
    }
    let _reset = Reset;
    let (first, cached) = crate::limits_refresh::test_saved_reader(&|_| {
        native_calls.fetch_add(1, Ordering::SeqCst);
        vec![current.clone()]
    });
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(native_calls.load(Ordering::SeqCst), 1);
    assert_eq!(first, cached);
    assert_eq!(first.len(), 2);
    assert_eq!(first[0], current);
    assert_eq!(first[1].reading.windows[0].used_percent, 42.0);
    assert!(!first[1].current_account);
}

// Thread-local I/O fixture: the production shared reader still binds and calls usage::refresh.
type RefreshFixture = Box<dyn Fn(bool, &mut Vec<ProviderLimitsDto>)>;
thread_local! {
    static TEST_REFRESH: std::cell::RefCell<Option<RefreshFixture>> = const { std::cell::RefCell::new(None) };
}
pub(super) fn refresh_fixture(force: bool, entries: &mut Vec<ProviderLimitsDto>) -> bool {
    TEST_REFRESH.with(|fixture| {
        let fixture = fixture.borrow();
        if let Some(refresh) = fixture.as_ref() {
            refresh(force, entries);
            true
        } else {
            false
        }
    })
}

#[test]
fn owned_renewal_uses_the_rotated_credential_for_usage() {
    use std::cell::Cell;
    for provider in [AgentId::Claude, AgentId::Codex] {
        for expired in [false, true] {
            let p = policy_profile(provider, true, expired);
            let fingerprint =
                |login: &Login| super::super::view(provider, login).unwrap().fingerprint();
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
                        Err(HttpError::Unauthorized)
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

/// Refreshes rebuild the cards from the snapshots on disk, so the count has to survive there, not
/// only in the card the first poll after switching away merges into.
#[test]
fn a_saved_poll_that_cannot_tell_keeps_the_remembered_banked_reset_count_across_refreshes() {
    use crate::dto::LimitsResetCreditsDto;
    use crate::limits::login::{remember, remembered};
    let home = tempfile::tempdir().unwrap();
    let p = stored(home.path());
    let banked = Some(LimitsResetCreditsDto {
        available_count: 1,
        next_expires_at: None,
    });
    let mut reading_with_count = reading(&p);
    reading_with_count.reading.reset_credits.clone_from(&banked);
    remember(home.path(), &reading_with_count).unwrap();

    for (refresh, observed_at) in ["2026-09-20T00:00:00Z", "2026-09-21T00:00:00Z"]
        .into_iter()
        .enumerate()
    {
        let mut entries = remembered(home.path(), AgentId::Claude);
        let result = poll_with(
            home.path(),
            &p,
            &ticket(home.path()),
            true,
            &|| Ok(open(home.path())),
            &|p| {
                let mut dto = reading(p);
                dto.reading.windows[0].observed_at = observed_at.into();
                FetchResult {
                    login: p.login.clone(),
                    result: Ok(dto),
                }
            },
        );
        merge(&mut entries, &p, result);
        assert_eq!(
            entries[0].reading.reset_credits, banked,
            "refresh {refresh}"
        );
    }
}

/// A saved read whose spending read failed or was backing off keeps the figure the card had.
#[test]
fn a_saved_read_that_could_not_tell_what_was_spent_keeps_the_cards_figure() {
    let profile = profile();
    let spent = Some(crate::dto::LimitsCreditsSpentDto {
        last_7_days: 18303.4,
        last_30_days: 20299.7,
        updated_at: None,
    });
    let mut entries = vec![];
    let mut first = codex_reading(&profile, "business");
    first.reading.credits_spent.clone_from(&spent);
    merge(&mut entries, &profile, Some(Ok(first)));

    merge(
        &mut entries,
        &profile,
        Some(Ok(codex_reading(&profile, "business"))),
    );

    assert_eq!(entries[0].reading.credits_spent, spent);
}

/// A saved read whose term read failed or was backing off keeps the term the card had.
#[test]
fn a_saved_read_that_could_not_tell_the_term_keeps_the_cards_term() {
    let profile = profile();
    let term = Some(crate::dto::LimitsSubscriptionDto {
        active_until: "2026-09-28T16:22:34Z".to_string(),
        will_renew: false,
        note: Some(crate::dto::SubscriptionNote::Cancelled),
        checked_at: "2026-09-25T12:00:00Z".to_string(),
    });
    let mut entries = vec![];
    let mut first = codex_reading(&profile, "pro");
    first.reading.subscription.clone_from(&term);
    merge(&mut entries, &profile, Some(Ok(first)));

    merge(
        &mut entries,
        &profile,
        Some(Ok(codex_reading(&profile, "pro"))),
    );

    assert_eq!(entries[0].reading.subscription, term);
}

/// A saved Codex read on `plan`.
fn codex_reading(profile: &Profile, plan: &str) -> ProviderLimitsDto {
    let mut dto = reading(profile);
    dto.provider = AgentId::Codex;
    dto.reading.plan = Some(plan.to_string());
    dto
}

/// An account now on a personal plan pools nothing and is never asked what it spent: the figure it
/// had on a workspace plan goes, rather than staying on its card for good.
#[test]
fn a_saved_personal_plan_read_drops_the_cards_figure() {
    let profile = profile();
    let mut entries = vec![];
    let mut first = codex_reading(&profile, "business");
    first.reading.credits_spent = Some(crate::dto::LimitsCreditsSpentDto {
        last_7_days: 18303.4,
        last_30_days: 20299.7,
        updated_at: None,
    });
    merge(&mut entries, &profile, Some(Ok(first)));

    merge(
        &mut entries,
        &profile,
        Some(Ok(codex_reading(&profile, "pro"))),
    );

    assert_eq!(entries[0].reading.credits_spent, None);
}

/// `profile` as a saved Codex account, and its card with every figure known, as the card list holds
/// it before a poll: remembered, not the signed-in account.
fn remembered_codex_card(profile: &mut Profile) -> ProviderLimitsDto {
    profile.identity.provider = AgentId::Codex;
    serde_json::from_value(json!({
        "provider": "codex",
        "status": "ok",
        "account": {"id": profile.identity.observation_key(), "label": "a@example.com"},
        "currentAccount": false,
        "plan": "business",
        "subscriptionStatus": "active",
        "windows": [
            {"id": "primary", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 40.0,
             "observedAt": "2026-09-19T00:00:00Z"}
        ],
        "credits": {"balance": "0", "unlimited": false},
        "workspaceCredits": {"limit": "25000", "used": "8000", "usedPercent": 32.0, "reached": false},
        "creditsSpent": {"last7Days": 18303.4, "last30Days": 20299.7},
        "subscription": {"activeUntil": "2100-09-28T16:22:34Z", "willRenew": false,
                         "checkedAt": "2026-09-19T00:00:00Z"},
        "resetCredits": {"availableCount": 1}
    }))
    .unwrap()
}

/// A failed poll shows the card's whole remembered reading under the failure, its term included.
#[test]
fn a_failed_saved_read_keeps_the_cards_whole_reading() {
    let mut profile = profile();
    let remembered = remembered_codex_card(&mut profile);
    let mut entries = vec![remembered.clone()];

    merge(&mut entries, &profile, Some(Err("paused".into())));

    let mut expected = serde_json::to_value(&remembered).unwrap();
    expected["status"] = json!("failed");
    expected["message"] = json!("paused");
    assert_eq!(entries.len(), 1);
    assert_eq!(serde_json::to_value(&entries[0]).unwrap(), expected);
}

/// A poll that answered with its plan and one window: the account details and balances it did
/// not report are gone, and only the figures it could not tell are kept.
#[test]
fn an_answered_saved_read_keeps_only_the_figures_it_could_not_tell() {
    let mut profile = profile();
    let remembered = remembered_codex_card(&mut profile);
    let mut entries = vec![remembered];
    let answered: ProviderLimitsDto = serde_json::from_value(json!({
        "provider": "codex",
        "status": "ok",
        "account": {"id": profile.identity.observation_key(), "label": "a@example.com"},
        "currentAccount": false,
        "plan": "business",
        "windows": [
            {"id": "primary", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 50.0,
             "observedAt": "2026-09-20T00:00:00Z"}
        ]
    }))
    .unwrap();

    merge(&mut entries, &profile, Some(Ok(answered)));

    assert_eq!(
        serde_json::to_value(&entries[0]).unwrap(),
        json!({
            "provider": "codex",
            "status": "ok",
            "account": {"id": profile.identity.observation_key(), "label": "a@example.com"},
            "currentAccount": false,
            "plan": "business",
            "windows": [
                {"id": "primary", "label": "Weekly · all models", "kind": "weekly",
                 "usedPercent": 50.0, "observedAt": "2026-09-20T00:00:00Z"}
            ],
            "creditsSpent": {"last7Days": 18303.4, "last30Days": 20299.7},
            "subscription": {"activeUntil": "2100-09-28T16:22:34Z", "willRenew": false,
                             "checkedAt": "2026-09-19T00:00:00Z"},
            "resetCredits": {"availableCount": 1}
        })
    );
}
