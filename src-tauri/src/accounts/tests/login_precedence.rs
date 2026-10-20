use super::super::{
    login_state::{Deadline, LoginState, SignInEnd, Standing},
    model::SignInDeadline,
    store::Profile,
};
use super::fixture::{claude, identity};
use crate::dto::AgentId;
use crate::limits::outcome::Parked;

const NOW_MS: i64 = 1_800_000_000_000;

#[derive(Clone, Copy, Debug)]
enum Date {
    None,
    Ahead,
    Now,
    Passed,
}

fn profile(saved_login: bool) -> Profile {
    let you = identity(AgentId::Claude, "you", "team");
    Profile {
        id: "saved-you".into(),
        identity: you,
        label: "you@example.com".into(),
        email: Some("you@example.com".into()),
        category: None,
        saved_at: "2026-10-01T00:00:00Z".into(),
        login: saved_login.then(|| claude("you", "y1")),
        pending_activation: false,
        usage_renewal_owned: false,
        home: None,
        signed_out: false,
        sign_in_end: SignInEnd::Unread,
    }
}

fn deadline(profile: &Profile, date: Date) -> Deadline {
    let ends_at_ms = match date {
        Date::None => return Deadline::of(profile, None, None),
        Date::Ahead => NOW_MS + 60_000,
        Date::Now => NOW_MS,
        Date::Passed => NOW_MS - 60_000,
    };
    let native = SignInDeadline {
        identity: profile.identity.clone(),
        ends_at_ms,
    };
    Deadline::of(profile, Some(&native), None)
}

fn spelled(ends_at_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ends_at_ms)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn dated(date: Date) -> Option<LoginState> {
    match date {
        Date::None => None,
        Date::Ahead => Some(LoginState::Ending {
            ends_at: spelled(NOW_MS + 60_000),
        }),
        Date::Now => Some(LoginState::Ended {
            ends_at: spelled(NOW_MS),
        }),
        Date::Passed => Some(LoginState::Ended {
            ends_at: spelled(NOW_MS - 60_000),
        }),
    }
}

fn expected_saved(saved_login: bool, date: Date, parked: Option<Parked>) -> LoginState {
    let ended = dated(date).filter(|state| matches!(state, LoginState::Ended { .. }));
    let ending = dated(date).filter(|state| matches!(state, LoginState::Ending { .. }));
    [
        (!saved_login).then_some(LoginState::Missing),
        ended,
        (parked == Some(Parked::OtherAccount)).then_some(LoginState::OtherAccount),
        (parked == Some(Parked::Refused)).then_some(LoginState::Refused),
        ending,
    ]
    .into_iter()
    .flatten()
    .next()
    .unwrap_or(LoginState::Usable)
}

#[test]
fn a_saved_login_is_missing_then_ended_then_another_account_then_refused_then_ending() {
    for saved_login in [true, false] {
        for date in [Date::None, Date::Ahead, Date::Now, Date::Passed] {
            for parked in [None, Some(Parked::Refused), Some(Parked::OtherAccount)] {
                let profile = profile(saved_login);
                let state = LoginState::of(
                    &profile,
                    &deadline(&profile, date),
                    Standing::Saved(parked),
                    NOW_MS,
                );
                assert_eq!(
                    state,
                    expected_saved(saved_login, date, parked),
                    "saved login {saved_login}, date {date:?}, parked {parked:?}"
                );
            }
        }
    }
}

#[test]
fn a_signed_in_login_is_its_date_alone() {
    for saved_login in [true, false] {
        for date in [Date::None, Date::Ahead, Date::Now, Date::Passed] {
            let profile = profile(saved_login);
            let state = LoginState::of(
                &profile,
                &deadline(&profile, date),
                Standing::SignedIn,
                NOW_MS,
            );
            assert_eq!(
                state,
                dated(date).unwrap_or(LoginState::Usable),
                "saved login {saved_login}, date {date:?}"
            );
        }
    }
}

#[test]
fn a_date_reached_this_very_millisecond_has_ended() {
    let profile = profile(true);
    assert_eq!(
        LoginState::of(
            &profile,
            &deadline(&profile, Date::Now),
            Standing::Saved(Some(Parked::Refused)),
            NOW_MS
        ),
        LoginState::Ended {
            ends_at: spelled(NOW_MS)
        }
    );
}
