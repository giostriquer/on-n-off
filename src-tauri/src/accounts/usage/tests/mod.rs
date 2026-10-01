use super::*;
mod merging;
mod provenance;
mod reading;
mod renewal;

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
                    result: Err(HttpError::RateLimited(RateLimitReset::RetryAfter(7200)).into()),
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
            result: Ok(reading(p))
        }
    )
    .unwrap()
    .is_ok());
}
#[test]
fn each_fetch_result_says_why_and_how_long_it_holds_the_next_poll_back() {
    const SIGN_IN: &str =
        "Usage refresh needs sign-in again or renewal by the client that owns this login.";
    const OTHER: &str = "This saved login now signs in as a different account. Sign in again.";
    const RATE: &str = "Usage refresh is rate limited. The last reading is retained.";
    const UNAVAILABLE: &str = "Usage refresh is unavailable. The last reading is retained.";
    let rate = |reset| Err(HttpError::RateLimited(reset).into());
    for (result, expected) in [
        (
            Ok(reading(&profile())),
            AttemptOutcome {
                error: None,
                rejected: false,
                retry: Duration::ZERO,
            },
        ),
        (
            Err(HttpError::Unauthorized.into()),
            AttemptOutcome::failed(SIGN_IN, true, Duration::ZERO),
        ),
        (
            Err(SavedReadError::OtherAccount),
            AttemptOutcome::failed(OTHER, true, Duration::ZERO),
        ),
        (
            Err(SavedReadError::Unavailable("Update Claude Code.")),
            AttemptOutcome::failed("Update Claude Code.", false, Duration::ZERO),
        ),
        (
            rate(RateLimitReset::RetryAfter(30)),
            AttemptOutcome::failed(RATE, false, Duration::from_secs(30)),
        ),
        (
            rate(RateLimitReset::RetryAfter(10_000_000)),
            AttemptOutcome::failed(RATE, false, Duration::from_secs(86400)),
        ),
        (
            rate(RateLimitReset::At(0)),
            AttemptOutcome::failed(RATE, false, Duration::ZERO),
        ),
        (
            rate(RateLimitReset::Unknown),
            AttemptOutcome::failed(RATE, false, Duration::ZERO),
        ),
        (
            Err(HttpError::Status(503).into()),
            AttemptOutcome::failed(UNAVAILABLE, false, Duration::ZERO),
        ),
        (
            Err(HttpError::Network("offline".into()).into()),
            AttemptOutcome::failed(UNAVAILABLE, false, Duration::ZERO),
        ),
    ] {
        assert_eq!(attempt_outcome(&result), expected, "{:?}", result.err());
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
fn a_poll_that_read_nothing_says_why() {
    use std::cell::Cell;
    for (error, message, sticky) in [
        (
            HttpError::Unauthorized.into(),
            "Usage refresh needs sign-in again or renewal by the client that owns this login.",
            true,
        ),
        (
            SavedReadError::OtherAccount,
            "This saved login now signs in as a different account. Sign in again.",
            true,
        ),
        (
            HttpError::RateLimited(RateLimitReset::RetryAfter(30)).into(),
            "Usage refresh is rate limited. The last reading is retained.",
            false,
        ),
        (
            HttpError::Status(503).into(),
            "Usage refresh is unavailable. The last reading is retained.",
            false,
        ),
        (
            HttpError::Network("offline".into()).into(),
            "Usage refresh is unavailable. The last reading is retained.",
            false,
        ),
    ] {
        let home = tempfile::tempdir().unwrap();
        let p = stored(home.path());
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
        assert_eq!(poll(), Some(Err(message.to_string())), "{error:?}");
        assert_eq!(attempt(home.path(), &p).rejected, sticky, "{error:?}");
        assert_eq!(poll(), Some(Err(message.to_string())), "{error:?}");
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
                result: Ok(reading(p)),
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
                result: Err(HttpError::Unauthorized.into()),
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
                    }
                    .into()),
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
                result: Ok(reading(p)),
            }
        },
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(entries[0].reading.windows[0].used_percent, 42.0);
}
