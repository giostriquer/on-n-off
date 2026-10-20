use super::super::{model::SignInDeadline, store::Login, usage::FetchResult};
use super::fixture::{claude, codex, identity, muse, provider_reading, Harness};
use crate::dto::{AgentId, LimitsStatus, ProviderLimitsDto};
use crate::http::RateLimitReset;
use crate::limits::ReadFailure;
use serde_json::{json, Value};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

const AHEAD_MS: i64 = 4_102_444_800_000;
const PASSED_MS: i64 = 1_577_836_800_000;

#[derive(Clone, Copy, Debug)]
enum Kept {
    Vault,
    Home,
}

fn you(provider: AgentId, generation: &str) -> (super::super::model::Identity, Login) {
    match provider {
        AgentId::Claude => (identity(provider, "you", "team"), claude("you", generation)),
        AgentId::Codex => (identity(provider, "you", "team"), codex("you", generation)),
        _ => (
            identity(provider, "you@example.com", ""),
            muse("you@example.com", generation),
        ),
    }
}

fn signed_in_elsewhere(provider: AgentId) -> Login {
    match provider {
        AgentId::Claude => claude("me", "m1"),
        AgentId::Codex => codex("me", "m1"),
        _ => muse("me@example.com", "m1"),
    }
}

const KINDS: [(AgentId, Kept); 5] = [
    (AgentId::Claude, Kept::Vault),
    (AgentId::Claude, Kept::Home),
    (AgentId::Codex, Kept::Vault),
    (AgentId::Muse, Kept::Vault),
    (AgentId::Muse, Kept::Home),
];

struct Saved {
    harness: Harness,
    provider: AgentId,
    kept: Kept,
    fetched: Mutex<usize>,
    answer: Mutex<Option<ReadFailure>>,
}

impl Saved {
    fn new(provider: AgentId, kept: Kept) -> Self {
        let harness = match kept {
            Kept::Vault => Harness::new(),
            Kept::Home => Harness::new().with_homes(),
        };
        let (identity, login) = you(provider, "g1");
        harness.saved(identity, login);
        harness.signed_in(Some(signed_in_elsewhere(provider)));
        Self {
            harness,
            provider,
            kept,
            fetched: Mutex::new(0),
            answer: Mutex::new(None),
        }
    }

    fn answering(&self, failure: Option<ReadFailure>) {
        if let Kept::Home = self.kept {
            self.harness
                .homes()
                .answer
                .lock()
                .unwrap()
                .clone_from(&failure);
        }
        *self.answer.lock().unwrap() = failure;
    }

    fn poll(&self, force: bool) -> bool {
        self.polled(force).0
    }

    fn polled(&self, force: bool) -> (bool, Vec<ProviderLimitsDto>) {
        let mut entries = Vec::new();
        let (fetched, answer, provider) = (&self.fetched, &self.answer, self.provider);
        let changed =
            self.harness
                .accounts()
                .refresh_usage(provider, force, &mut entries, &|profile, _| {
                    *fetched.lock().unwrap() += 1;
                    FetchResult {
                        login: profile.login.clone(),
                        result: match answer.lock().unwrap().clone() {
                            Some(failure) => Err(failure),
                            None => Ok(crate::limits::Accepted::for_test(provider_reading(
                                provider,
                                &profile.identity.observation_key(),
                            ))),
                        },
                    }
                });
        (changed, entries)
    }

    fn card(&self, force: bool) -> Option<ProviderLimitsDto> {
        let (identity, _) = you(self.provider, "g1");
        let key = identity.observation_key();
        self.polled(force).1.into_iter().find(|card| {
            card.account
                .as_ref()
                .is_some_and(|account| account.id == key)
        })
    }

    fn context(&self) -> crate::limits::outcome::Saved {
        match self.kept {
            Kept::Vault => crate::limits::outcome::Saved::Login,
            Kept::Home => crate::limits::outcome::Saved::Home,
        }
    }

    fn reads(&self) -> usize {
        match self.kept {
            Kept::Vault => *self.fetched.lock().unwrap(),
            Kept::Home => self.harness.homes().read.lock().unwrap().len(),
        }
    }

    fn sign_in_again(&self, generation: &str) {
        let (_, login) = you(self.provider, generation);
        match self.kept {
            Kept::Vault => self.harness.seed(|db| db.profiles[0].login = Some(login)),
            Kept::Home => {
                let id = self.harness.vault().profiles[0].id.clone();
                let dir = self.harness.home_of(&id).expect("the login is in its home");
                self.harness
                    .homes()
                    .logins
                    .lock()
                    .unwrap()
                    .insert(dir, login);
            }
        }
    }

    fn state(&self) -> Value {
        login_states(&self.harness, self.provider).remove(0)
    }
}

fn login_states(harness: &Harness, provider: AgentId) -> Vec<Value> {
    let listed = serde_json::to_value(harness.accounts().list(provider).unwrap()).unwrap();
    listed["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|profile| profile["loginState"].clone())
        .collect()
}

fn dated(mut login: Login, ends_at_ms: i64) -> Login {
    login.auth["claudeAiOauth"]["refreshTokenExpiresAt"] = ends_at_ms.into();
    login
}

#[test]
fn a_refused_login_waits_for_a_new_one_wherever_it_is_kept() {
    for (provider, kept) in KINDS {
        let saved = Saved::new(provider, kept);
        saved.answering(Some(ReadFailure::LoginRefused));
        saved.poll(false);
        assert_eq!(saved.reads(), 1, "{provider:?} in {kept:?}");

        super::super::usage::tests::elapse(saved.harness.path());
        saved.poll(true);
        assert_eq!(
            saved.reads(),
            1,
            "{provider:?} in {kept:?} read a refused login again before it changed"
        );

        saved.answering(None);
        saved.sign_in_again("g2");
        saved.poll(false);
        assert_eq!(
            saved.reads(),
            2,
            "{provider:?} in {kept:?} waits on after its login changed"
        );
    }
}

#[test]
fn every_saved_poll_honours_a_retry_after_and_backs_off_the_same_way() {
    let interval = crate::limits_refresh::poll_interval();
    for (failure, wait) in [
        (
            ReadFailure::RateLimited(RateLimitReset::RetryAfter(7200)),
            Duration::from_secs(7200),
        ),
        (
            ReadFailure::RequestFailed("status 503".into()),
            crate::limits_refresh::failure_backoff(interval, 1),
        ),
    ] {
        for (provider, kept) in KINDS {
            let saved = Saved::new(provider, kept);
            saved.answering(Some(failure.clone()));
            let start = Instant::now();
            saved.poll(false);
            let end = Instant::now();
            let held = saved.card(true).expect("a held poll keeps its card");
            assert_eq!(
                saved.reads(),
                1,
                "{failure:?}: {provider:?} in {kept:?} forced past its hold"
            );
            assert_eq!(
                held.status,
                LimitsStatus::Failed,
                "{provider:?} in {kept:?}"
            );
            assert_eq!(
                held.message,
                Some(crate::limits::outcome::saved(provider, saved.context(), &failure, 0).message),
                "{failure:?}: {provider:?} in {kept:?} replays why it is held"
            );
            let [(next, failures)] = super::super::usage::tests::holds(saved.harness.path())[..]
            else {
                panic!("one attempt recorded");
            };
            assert_eq!(failures, 1);
            assert!(
                next >= start + wait && next <= end + wait,
                "{failure:?}: {provider:?} in {kept:?} waits {wait:?}"
            );
            assert_eq!(
                saved.state(),
                json!({"state": "usable"}),
                "{provider:?} in {kept:?}"
            );

            super::super::usage::tests::elapse(saved.harness.path());
            saved.poll(false);
            assert_eq!(
                saved.reads(),
                2,
                "{failure:?}: {provider:?} in {kept:?} parked"
            );
        }
    }
}

#[test]
fn a_poll_says_whether_a_saved_login_now_refuses_or_answers_for_another_account() {
    let cases = [
        (ReadFailure::LoginRefused, json!({"state": "refused"})),
        (ReadFailure::SignedOut, json!({"state": "refused"})),
        (ReadFailure::OtherAccount, json!({"state": "otherAccount"})),
        (
            ReadFailure::RequestFailed("status 503".into()),
            json!({"state": "usable"}),
        ),
        (
            ReadFailure::Unavailable("The source said why.".into()),
            json!({"state": "usable"}),
        ),
    ];
    for (provider, kept) in KINDS {
        for (failure, expected) in &cases {
            let saved = Saved::new(provider, kept);
            assert_eq!(
                saved.state(),
                json!({"state": "usable"}),
                "{provider:?} in {kept:?}"
            );
            saved.answering(Some(failure.clone()));
            saved.poll(false);
            assert_eq!(
                &saved.state(),
                expected,
                "{failure:?} for {provider:?} in {kept:?}"
            );
        }
    }
}

#[test]
fn a_new_login_is_usable_again_and_the_poll_says_the_state_changed() {
    for (provider, kept) in KINDS {
        let saved = Saved::new(provider, kept);
        assert!(
            !saved.poll(false),
            "{provider:?} in {kept:?}: nothing changed"
        );
        super::super::usage::tests::elapse(saved.harness.path());
        saved.answering(Some(ReadFailure::OtherAccount));
        assert!(
            saved.poll(true),
            "{provider:?} in {kept:?}: now another account"
        );
        assert!(!saved.poll(true), "{provider:?} in {kept:?}: still parked");
        assert_eq!(saved.state(), json!({"state": "otherAccount"}));

        saved.answering(None);
        saved.sign_in_again("g2");
        assert_eq!(
            saved.state(),
            json!({"state": "usable"}),
            "{provider:?} in {kept:?}"
        );
        assert!(saved.poll(false), "{provider:?} in {kept:?}: usable again");
    }
}

#[test]
fn a_profile_with_neither_a_login_nor_a_home_is_missing() {
    for provider in [AgentId::Claude, AgentId::Codex, AgentId::Muse] {
        let saved = Saved::new(provider, Kept::Vault);
        saved.harness.seed(|db| db.profiles[0].login = None);
        assert_eq!(saved.state(), json!({"state": "missing"}), "{provider:?}");
    }
}

#[test]
fn a_sign_in_with_a_deadline_ahead_is_ending_and_one_past_it_has_ended() {
    for kept in [Kept::Vault, Kept::Home] {
        for (ends_at_ms, expected) in [
            (
                AHEAD_MS,
                json!({"state": "ending", "endsAt": "2100-01-01T00:00:00Z"}),
            ),
            (
                PASSED_MS,
                json!({"state": "ended", "endsAt": "2020-01-01T00:00:00Z"}),
            ),
        ] {
            let saved = Saved::new(AgentId::Claude, kept);
            saved
                .harness
                .seed(|db| db.profiles[0].login = Some(dated(claude("you", "g1"), ends_at_ms)));
            saved.poll(false);
            assert_eq!(saved.state(), expected, "{kept:?}");
        }
    }
}

#[test]
fn an_ended_sign_in_outranks_the_refusal_it_brings() {
    let saved = Saved::new(AgentId::Claude, Kept::Home);
    saved
        .harness
        .seed(|db| db.profiles[0].login = Some(dated(claude("you", "g1"), PASSED_MS)));
    saved.answering(Some(ReadFailure::SignedOut));
    saved.poll(false);
    assert_eq!(
        saved.state(),
        json!({"state": "ended", "endsAt": "2020-01-01T00:00:00Z"})
    );
}

#[test]
fn the_signed_in_accounts_state_is_its_deadline_alone() {
    let harness = Harness::new();
    let (identity, login) = you(AgentId::Claude, "g1");
    harness.saved(identity.clone(), login.clone());
    harness.signed_in(Some(login));
    *harness.native.deadline.borrow_mut() = Some(SignInDeadline {
        identity,
        ends_at_ms: AHEAD_MS,
    });

    assert_eq!(
        login_states(&harness, AgentId::Claude),
        [json!({"state": "ending", "endsAt": "2100-01-01T00:00:00Z"})]
    );
}

#[test]
fn a_home_whose_login_cannot_be_stamped_keeps_its_verdict_and_is_not_read_again() {
    let saved = Saved::new(AgentId::Muse, Kept::Home);
    saved.answering(Some(ReadFailure::LoginRefused));
    saved.poll(false);
    let id = saved.harness.vault().profiles[0].id.clone();
    let dir = saved.harness.home_of(&id).unwrap();
    saved.harness.homes().unreadable.lock().unwrap().push(dir);

    super::super::usage::tests::elapse(saved.harness.path());
    saved.answering(None);
    assert!(!saved.poll(true), "nothing new is known");

    assert_eq!(
        saved.reads(),
        1,
        "read again without knowing its login changed"
    );
    assert_eq!(saved.state(), json!({"state": "refused"}));
}

#[test]
fn a_signed_in_account_without_a_saved_login_still_says_when_its_sign_in_ends() {
    for (ends_at_ms, expected) in [
        (
            AHEAD_MS,
            json!({"state": "ending", "endsAt": "2100-01-01T00:00:00Z"}),
        ),
        (
            PASSED_MS,
            json!({"state": "ended", "endsAt": "2020-01-01T00:00:00Z"}),
        ),
    ] {
        let harness = Harness::new();
        let (identity, login) = you(AgentId::Claude, "g1");
        harness.saved(identity.clone(), login.clone());
        harness.seed(|db| db.profiles[0].login = None);
        harness.signed_in(Some(login));
        *harness.native.deadline.borrow_mut() = Some(SignInDeadline {
            identity,
            ends_at_ms,
        });

        assert_eq!(login_states(&harness, AgentId::Claude), [expected]);
    }
}

#[test]
fn every_login_state_is_written_as_the_card_reads_it() {
    use super::super::login_state::LoginState;
    let written = serde_json::to_value([
        LoginState::Usable,
        LoginState::Ending {
            ends_at: "2100-01-01T00:00:00Z".into(),
        },
        LoginState::Ended {
            ends_at: "2020-01-01T00:00:00Z".into(),
        },
        LoginState::Refused,
        LoginState::OtherAccount,
        LoginState::Missing,
    ])
    .unwrap();

    assert_eq!(
        written,
        serde_json::from_str::<Value>(include_str!("../fixtures/login-states.json")).unwrap(),
        "src-tauri/src/accounts/fixtures/login-states.json must read:\n{}",
        serde_json::to_string_pretty(&written).unwrap()
    );
}
