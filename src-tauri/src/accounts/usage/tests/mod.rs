use super::*;
mod merging;
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
                    result: Err(HttpError::RateLimited(RateLimitReset::RetryAfter(1800)).into()),
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
/// What a poll that read nothing says, and whether it waits for a new login before it reads again.
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
            SavedReadError::Expired,
            "This saved login has expired. Use this account once, or sign in again, to renew it.",
            false,
        ),
        (
            HttpError::RateLimited(RateLimitReset::RetryAfter(30)).into(),
            "Usage refresh is rate limited. The last reading is retained.",
            false,
        ),
        (
            HttpError::Status(429).into(),
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
        let state = ATTEMPTS
            .get()
            .and_then(|attempts| {
                let key = format!("{}:{}", home.path().display(), p.id);
                attempts.lock().unwrap().get(&key).cloned()
            })
            .expect("the attempt is recorded");
        assert_eq!(state.rejected, sticky, "{error:?}");
        assert_eq!(poll(), Some(Err(message.to_string())), "{error:?}");
        assert_eq!(fetches.get(), 1, "{error:?}");
    }
}

/// A reading the snapshot store could not keep is shown as a failed poll without its numbers, and
/// the poll counts as a success.
#[test]
fn a_reading_that_could_not_be_saved_is_shown_as_a_failed_poll() {
    let home = tempfile::tempdir().unwrap();
    let p = stored(home.path());
    // A file where the snapshot directory goes: every snapshot write fails.
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
    assert_eq!(
        poll(),
        Some(Err("Could not save the latest usage reading.".to_string()))
    );
    assert_eq!(
        poll(),
        None,
        "recorded as a success, so it waits out the interval"
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

/// A CLI signed in with no subscription login, an API key among them
/// (`codex::CodexNative::subscription`), excludes no saved account from polling.
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
                Ok(Some(active.identity.clone())),
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
