use super::*;
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
    entries[0].windows.push(crate::dto::LimitWindowDto {
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
    assert_eq!(entries[0].windows[0].used_percent, 73.0);
    assert_eq!(entries[0].windows[0].observed_at, "2026-09-01T00:00:00Z");
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
        dto.reset_credits = reset_credits;
        Some(Ok(dto))
    };
    let mut entries = vec![];

    merge(&mut entries, &profile, read(banked(1)));
    merge(&mut entries, &profile, read(None));
    assert_eq!(entries[0].reset_credits, banked(1));

    merge(&mut entries, &profile, read(banked(0)));
    assert_eq!(entries[0].reset_credits, banked(0));
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
    let store = open(home);
    let mut db = super::super::store::Database::default();
    let mut p = profile();
    p.login = Some(Login {
        auth: json!({"claudeAiOauth":{"accessToken":"old","refreshToken":"refresh"}}),
        account: json!({"accountUuid":"user","organizationUuid":"team"}),
    });
    db.profiles.push(p.clone());
    store.persist(&db).unwrap();
    p
}
fn open(home: &Path) -> Store {
    Store::open_with_key(home, true, |_, _| Ok([8; 32])).unwrap()
}
fn reading(profile: &Profile) -> ProviderLimitsDto {
    let mut entries = vec![];
    merge(&mut entries, profile, Some(Err("fixture".into())));
    let mut dto = entries.remove(0);
    dto.status = LimitsStatus::Ok;
    dto.message = None;
    dto.windows = vec![crate::dto::LimitWindowDto {
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
    let result = poll_with(home.path(), &p, 0, false, &|| Ok(open(home.path())), &|p| {
        FetchResult {
            login: p.login.clone(),
            result: Ok(reading(p)),
        }
    })
    .unwrap()
    .unwrap();
    assert_eq!(result.windows[0].used_percent, 42.0);
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
        let result = poll_with(home.path(), &p, 0, false, &|| Ok(open(home.path())), &|p| {
            let store = open(home.path());
            let mut db = store.load().unwrap();
            if remove {
                db.profiles.clear();
            } else {
                db.profiles[0].login.as_mut().unwrap().auth["claudeAiOauth"]["accessToken"] =
                    json!("replacement");
            }
            db.invalidate_logins().unwrap();
            store.persist(&db).unwrap();
            FetchResult {
                login: p.login.clone(),
                result: Ok(reading(p)),
            }
        });
        assert!(result.is_none());
        assert!(!home.path().join(".on-n-off/limits").exists());
    }
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
            0,
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
    let store = open(home.path());
    let mut db = store.load().unwrap();
    db.profiles.push(other.clone());
    store.persist(&db).unwrap();
    drop(store);
    assert!(poll_with(
        home.path(),
        &other,
        0,
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
    assert!(
        poll_with(home.path(), &p, 0, false, &|| Ok(open(home.path())), &|p| {
            FetchResult {
                login: p.login.clone(),
                result: Err(HttpError::Unauthorized),
            }
        })
        .unwrap()
        .is_err()
    );
    p.login.as_mut().unwrap().auth["claudeAiOauth"]["accessToken"] = json!("new-access");
    let store = open(home.path());
    let mut db = store.load().unwrap();
    db.profiles[0] = p.clone();
    store.persist(&db).unwrap();
    drop(store);
    assert!(
        poll_with(home.path(), &p, 0, false, &|| Ok(open(home.path())), &|p| {
            FetchResult {
                login: p.login.clone(),
                result: Ok(reading(p)),
            }
        })
        .unwrap()
        .is_ok()
    );
}

#[test]
fn post_rotation_failures_keep_the_new_generation_backoff() {
    for rejected in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let p = stored(home.path());
        let result = poll_with(
            home.path(),
            &p,
            0,
            false,
            &|| Ok(open(home.path())),
            &|_| {
                let store = open(home.path());
                let mut db = store.load().unwrap();
                let login = db.profiles[0].login.as_mut().unwrap();
                login.auth["claudeAiOauth"]["accessToken"] = json!("rotated");
                let login = login.clone();
                store.persist(&db).unwrap();
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
        let result = poll_with(home.path(), &p, 0, true, &|| Ok(open(home.path())), &|_| {
            panic!("forced refresh must retain the renewed generation's backoff")
        });
        assert!(result.unwrap().is_err());
    }
}

#[test]
fn native_api_key_login_does_not_block_saved_subscription_polling() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let home = tempfile::tempdir().unwrap();
    let mut p = stored(home.path());
    p.identity.provider = AgentId::Codex;
    let store = open(home.path());
    let mut db = store.load().unwrap();
    db.profiles[0] = p.clone();
    store.persist(&db).unwrap();
    drop(store);
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
    assert_eq!(entries[0].windows[0].used_percent, 42.0);
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

#[test]
fn shared_limits_reader_polls_inactive_accounts_once_and_preserves_active_results() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let home = tempfile::tempdir().unwrap();
    let inactive = stored(home.path());
    let mut active = inactive.clone();
    active.id = "active-profile".into();
    active.identity.user_id = "active-user".into();
    active.login.as_mut().unwrap().account["accountUuid"] = json!("active-user");
    let store = open(home.path());
    let mut db = store.load().unwrap();
    db.profiles.push(active.clone());
    store.persist(&db).unwrap();
    drop(store);
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let native_calls = AtomicUsize::new(0);
    let mut current = reading(&active);
    current.current_account = true;
    current.windows[0].used_percent = 19.0;
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
    assert_eq!(first[1].windows[0].used_percent, 42.0);
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
            let old = p.login.as_ref().unwrap().fingerprint();
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
                    if login.fingerprint() == old {
                        Err(HttpError::Unauthorized)
                    } else {
                        assert_eq!(login.fingerprint(), rotated.fingerprint());
                        Ok(reading(&p))
                    }
                },
                &|| Ok(rotated.clone()),
            );
            assert!(result.result.is_ok());
            assert_eq!(result.login.unwrap().fingerprint(), rotated.fingerprint());
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
    reading_with_count.reset_credits.clone_from(&banked);
    remember(home.path(), &reading_with_count).unwrap();

    for (refresh, observed_at) in ["2026-09-20T00:00:00Z", "2026-09-21T00:00:00Z"]
        .into_iter()
        .enumerate()
    {
        let mut entries = remembered(home.path(), AgentId::Claude);
        let result = poll_with(home.path(), &p, 0, true, &|| Ok(open(home.path())), &|p| {
            let mut dto = reading(p);
            dto.windows[0].observed_at = observed_at.into();
            FetchResult {
                login: p.login.clone(),
                result: Ok(dto),
            }
        });
        merge(&mut entries, &p, result);
        assert_eq!(entries[0].reset_credits, banked, "refresh {refresh}");
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
    first.credits_spent.clone_from(&spent);
    merge(&mut entries, &profile, Some(Ok(first)));

    merge(
        &mut entries,
        &profile,
        Some(Ok(codex_reading(&profile, "business"))),
    );

    assert_eq!(entries[0].credits_spent, spent);
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
    first.subscription.clone_from(&term);
    merge(&mut entries, &profile, Some(Ok(first)));

    merge(
        &mut entries,
        &profile,
        Some(Ok(codex_reading(&profile, "pro"))),
    );

    assert_eq!(entries[0].subscription, term);
}

/// A saved Codex read on `plan`.
fn codex_reading(profile: &Profile, plan: &str) -> ProviderLimitsDto {
    ProviderLimitsDto {
        provider: AgentId::Codex,
        plan: Some(plan.to_string()),
        ..reading(profile)
    }
}

/// An account now on a personal plan pools nothing and is never asked what it spent: the figure it
/// had on a workspace plan goes, rather than staying on its card for good.
#[test]
fn a_saved_personal_plan_read_drops_the_cards_figure() {
    let profile = profile();
    let mut entries = vec![];
    let mut first = codex_reading(&profile, "business");
    first.credits_spent = Some(crate::dto::LimitsCreditsSpentDto {
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

    assert_eq!(entries[0].credits_spent, None);
}
