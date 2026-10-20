use super::*;
mod merging;
mod provenance;
mod reading;
mod renewal;

use crate::accounts::{
    model,
    store::{ChangeKind, Database},
};
use crate::http::RateLimitReset;
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
fn rewrite(home: &Path, edit: impl FnOnce(&mut Database)) {
    open(home)
        .change(ChangeKind::Metadata, |db| {
            edit(db);
            Ok(())
        })
        .unwrap();
}
fn account_change(home: &Path, edit: impl FnOnce(&mut Database)) {
    open(home)
        .change(ChangeKind::Account, |db| {
            edit(db);
            Ok(())
        })
        .unwrap();
}
fn ticket(home: &Path) -> Ticket {
    open(home).load().unwrap().ticket(Guard::SignIn).unwrap()
}
fn answered(profile: &Profile) -> Accepted {
    Accepted::for_test(reading(profile))
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
            result: Ok(answered(p)),
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
                result: Ok(answered(p)),
            }
        },
    );
    assert!(result.is_none());
    assert!(!home.path().join(".on-n-off/limits").exists());
}
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
                result: Ok(answered(p)),
            }
        },
    );
    assert!(result.is_none());
    assert!(!home.path().join(".on-n-off/limits").exists());
    assert!(attempt(home.path(), &p).next > Instant::now());
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
                result: Ok(answered(p)),
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
                    result: Err(ReadFailure::RateLimited(RateLimitReset::RetryAfter(7200))),
                }
            },
        );
        assert!(result.unwrap().is_err());
    }
    assert_eq!(calls.get(), 1);
    assert!(
        attempt(home.path(), &p).next >= Instant::now() + Duration::from_secs(7200 - 60),
        "the next poll waits out the Retry-After"
    );
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
            result: Ok(answered(p))
        }
    )
    .unwrap()
    .is_ok());
}
#[test]
fn a_poll_holds_back_as_the_verdict_of_its_read_context_says() {
    let p = profile();
    for failure in [
        ReadFailure::LoginRefused,
        ReadFailure::OtherAccount,
        ReadFailure::RateLimited(RateLimitReset::RetryAfter(30)),
        ReadFailure::Unavailable("The source said why.".into()),
    ] {
        for saved in [Saved::Login, Saved::Home] {
            assert_eq!(
                verdict_of(saved, &p, &Err(failure.clone())),
                Some(outcome::saved(p.identity.provider, saved, &failure, 0)),
                "{failure:?} in {saved:?}"
            );
        }
    }
    assert_eq!(verdict_of(Saved::Login, &p, &Ok(answered(&p))), None);
}

#[test]
fn a_saved_poll_that_keeps_failing_waits_the_limits_failure_backoff_or_its_retry_after() {
    let failed = |retry| {
        Some(SavedVerdict {
            message: "failed".into(),
            parks_login: false,
            retry: Duration::from_secs(retry),
        })
    };
    let succeeded = None;
    for interval in [Duration::from_secs(60), Duration::from_secs(300)] {
        let backoff = |failures| crate::limits_refresh::failure_backoff(interval, failures);
        for (before, outcome, failures, wait) in [
            (0, failed(0), 1, backoff(1)),
            (1, failed(0), 2, backoff(2)),
            (2, failed(0), 3, backoff(3)),
            (4, failed(0), 5, backoff(5)),
            (3, succeeded.clone(), 0, backoff(0)),
            (0, failed(30), 1, backoff(1)),
            (0, failed(7200), 1, Duration::from_secs(7200)),
        ] {
            let attempts = Mutex::default();
            let previous = Attempt {
                fingerprint: "login".into(),
                next: Instant::now(),
                failures: before,
                error: Some("failed".into()),
                rejected: false,
            };
            let start = Instant::now();
            hold_back(
                &attempts,
                "key".into(),
                "login".into(),
                (before > 0).then_some(&previous),
                outcome,
                interval,
            );
            let end = Instant::now();
            let recorded = attempts.lock().unwrap()["key"].clone();
            assert_eq!(recorded.failures, failures);
            assert!(
                recorded.next >= start + wait && recorded.next <= end + wait,
                "{failures} failures at {interval:?} wait {wait:?}"
            );
        }
    }
}

fn attempt(home: &Path, p: &Profile) -> Attempt {
    ATTEMPTS
        .get()
        .and_then(|attempts| {
            let key = format!("{}:{}", home.display(), p.id);
            attempts.lock().unwrap().get(&key).cloned()
        })
        .expect("the attempt is recorded")
}

#[test]
fn a_poll_that_read_nothing_says_why_and_replays_it_while_held_back() {
    use std::cell::Cell;
    for error in [
        ReadFailure::LoginRefused,
        ReadFailure::OtherAccount,
        ReadFailure::RateLimited(RateLimitReset::RetryAfter(30)),
        ReadFailure::Unavailable("The source said why.".into()),
    ] {
        let home = tempfile::tempdir().unwrap();
        let p = stored(home.path());
        let verdict = outcome::saved(p.identity.provider, Saved::Login, &error, 0);
        let fetches = Cell::new(0);
        let poll = || {
            poll_with(
                home.path(),
                &p,
                &ticket(home.path()),
                false,
                &|| Ok(open(home.path())),
                &|p| {
                    fetches.set(fetches.get() + 1);
                    FetchResult {
                        login: p.login.clone(),
                        result: Err(error.clone()),
                    }
                },
            )
        };
        assert_eq!(poll(), Some(Err(verdict.message.clone())), "{error:?}");
        assert_eq!(
            attempt(home.path(), &p).rejected,
            verdict.parks_login,
            "{error:?}"
        );
        assert_eq!(poll(), Some(Err(verdict.message)), "{error:?}");
        assert_eq!(fetches.get(), 1, "{error:?}");
    }
}

#[test]
fn a_reading_that_could_not_be_saved_is_shown_and_retried() {
    let home = tempfile::tempdir().unwrap();
    let p = stored(home.path());
    std::fs::write(home.path().join(".on-n-off/limits"), "").unwrap();
    let poll = || {
        poll_with(
            home.path(),
            &p,
            &ticket(home.path()),
            false,
            &|| Ok(open(home.path())),
            &|p| FetchResult {
                login: p.login.clone(),
                result: Ok(answered(p)),
            },
        )
    };
    let message = "Could not save the latest usage reading.";
    let card = poll().expect("a card").expect("the fresh reading");
    assert_eq!(card.status, LimitsStatus::Failed);
    assert_eq!(card.message.as_deref(), Some(message));
    assert_eq!(card.reading.windows, reading(&p).reading.windows);
    let recorded = attempt(home.path(), &p);
    assert_eq!(recorded.error.as_deref(), Some(message));
    assert!(
        !recorded.rejected,
        "a new login is not needed to read again"
    );
    assert_eq!(recorded.failures, 1);
    assert_eq!(
        poll(),
        Some(Err(message.to_string())),
        "recorded as a failure, so it backs off and reads again"
    );
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
                result: Err(ReadFailure::LoginRefused),
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
                result: Ok(answered(p)),
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
                        ReadFailure::LoginRefused
                    } else {
                        ReadFailure::RateLimited(RateLimitReset::RetryAfter(1800))
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
fn a_cli_without_a_subscription_login_excludes_no_saved_account_from_polling() {
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
        Ok(None),
        &|| Ok(open(home.path())),
        &|p| {
            calls.fetch_add(1, Ordering::SeqCst);
            FetchResult {
                login: p.login.clone(),
                result: Ok(answered(p)),
            }
        },
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(entries[0].reading.windows[0].used_percent, 42.0);
}
