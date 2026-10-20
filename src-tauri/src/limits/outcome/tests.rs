use super::*;
use crate::dto::LimitWindowKind;
use crate::limits::window::fixture as window;

const NOW: i64 = 1_800_000_000;
const LOGIN: Context = Context::Saved(Saved::Login);
const HOME: Context = Context::Saved(Saved::Home);

pub(in crate::limits) fn every_failure_keeps_the_account_its_answer_names(
    provider: AgentId,
    answered: Read,
    failing: Vec<Read>,
) {
    let identity = |card: &ProviderLimitsDto| {
        card.account
            .as_ref()
            .map(|account| (account.id.clone(), account.legacy_id.clone()))
    };
    let (card, _) = signed_in(provider, answered);
    assert_eq!(card.status, LimitsStatus::Ok, "{:?}", card.message);
    let account = identity(&card).expect("an answer names its account");
    assert!(!failing.is_empty());
    for read in failing {
        let case = format!("{:?}", read.answer.as_ref().err());
        let (card, _) = signed_in(provider, read);
        assert_ne!(card.status, LimitsStatus::Ok, "{case}");
        assert_eq!(
            identity(&card),
            Some(account.clone()),
            "{case}: {:?}",
            card.message
        );
    }
}

fn failures() -> Vec<ReadFailure> {
    vec![
        ReadFailure::NotInstalled,
        ReadFailure::SignedOut,
        ReadFailure::Unsupported(Unsupported::Inactive),
        ReadFailure::LoginRefused,
        ReadFailure::OtherAccount,
        ReadFailure::AccountChanged,
        ReadFailure::RateLimited(RateLimitReset::RetryAfter(30)),
        ReadFailure::RequestFailed("HTTP 503".to_string()),
        ReadFailure::NoUsage,
        ReadFailure::Unavailable("The source said why.".to_string()),
    ]
}

#[test]
fn one_table_words_every_failure_in_every_context() {
    use ReadFailure as F;
    const INACTIVE: &str =
        "This Muse subscription is inactive, so it has no subscription limits to show.";
    const LIMITED: &str =
        "Meta is limiting how often Muse usage can be read. on-n-off will read it again later.";
    const REQUEST: &str =
        "The Muse usage read failed (HTTP 503). on-n-off will read it again later.";
    const NO_USAGE: &str = "Muse Code reported no usage.";
    const WHY: &str = "The source said why.";
    const VAULT_ENDED: &str =
        "Usage refresh needs sign-in again or renewal by the client that owns this login.";
    const HOME_ENDED: &str = "This account's saved login has ended. Sign in again.";
    const OTHER: &str = "This saved login now signs in as a different account. Sign in again.";
    const CHANGED: &str = "This account changed while its usage was read.";
    let rows: Vec<(Context, Vec<&str>)> = vec![
        (
            Context::SignedIn,
            vec![
                "Install Muse Code and sign in with `muse` to see subscription limits.",
                "Sign in with `muse` to see subscription limits.",
                INACTIVE,
                "Muse's login has ended. Sign in with `muse` again to see subscription limits.",
                "Muse answered for a different account. Sign in with `muse` again to see subscription limits.",
                "The signed-in Muse account changed while its usage was read.",
                LIMITED,
                REQUEST,
                NO_USAGE,
                WHY,
            ],
        ),
        (
            LOGIN,
            vec![
                "Muse Code is not installed.",
                VAULT_ENDED,
                INACTIVE,
                VAULT_ENDED,
                OTHER,
                CHANGED,
                LIMITED,
                REQUEST,
                NO_USAGE,
                WHY,
            ],
        ),
        (
            HOME,
            vec![
                "Muse Code is not installed.",
                HOME_ENDED,
                INACTIVE,
                HOME_ENDED,
                OTHER,
                CHANGED,
                LIMITED,
                REQUEST,
                NO_USAGE,
                WHY,
            ],
        ),
    ];
    for (context, words) in rows {
        assert_eq!(words.len(), failures().len(), "{context:?}");
        for (failure, expected) in failures().into_iter().zip(words) {
            assert_eq!(
                message(AgentId::Muse, context, &failure),
                expected,
                "{failure:?} in {context:?}"
            );
        }
    }
    for failure in failures() {
        let expected = if failure == F::OtherAccount {
            "Muse answered for a different account. Sign in again."
        } else {
            "The Muse login could not be verified. Check connectivity or sign in again."
        };
        assert_eq!(
            message(AgentId::Muse, Context::Verify, &failure),
            expected,
            "{failure:?}"
        );
    }
}

#[test]
fn a_signed_in_card_reads_each_failure_with_its_status_account_and_backoff() {
    use LimitsStatus::{Failed, SignedOut, Unsupported as NoLimits};
    let statuses = [
        (SignedOut, false),
        (SignedOut, false),
        (NoLimits, true),
        (Failed, true),
        (Failed, true),
        (Failed, true),
        (Failed, true),
        (Failed, true),
        (Failed, true),
        (Failed, true),
    ];
    let known = answered("profile:you", Reading::default()).account;
    for (failure, (status, keeps)) in failures().into_iter().zip(statuses) {
        let (card, outcome) = signed_in(
            AgentId::Muse,
            Read {
                account: known.clone(),
                answer: Err(failure.clone()),
            },
        );
        assert_eq!(card.status, status, "{failure:?}");
        assert_eq!(
            card.message,
            Some(message(AgentId::Muse, Context::SignedIn, &failure)),
            "{failure:?}"
        );
        assert_eq!(card.account.is_some(), keeps, "{failure:?}");
        assert!(card.current_account, "{failure:?}");
        assert!(!card.reading.has_observations(), "{failure:?}");
        assert_eq!(
            outcome.counts_toward_backoff(),
            status == Failed,
            "{failure:?}"
        );
        assert_eq!(outcome.failure, Some(failure.clone()));
        assert_eq!(outcome.login.as_deref(), Some("profile:you"), "{failure:?}");
    }
}

#[test]
fn a_saved_read_parks_only_a_vault_login_that_ended_or_signs_in_as_another() {
    for failure in failures() {
        let parks = matches!(
            failure,
            ReadFailure::SignedOut | ReadFailure::LoginRefused | ReadFailure::OtherAccount
        );
        let retry = Duration::from_secs(if matches!(failure, ReadFailure::RateLimited(_)) {
            30
        } else {
            0
        });
        for (which, context, parks) in [(Saved::Login, LOGIN, parks), (Saved::Home, HOME, false)] {
            assert_eq!(
                saved(AgentId::Muse, which, &failure, NOW),
                SavedVerdict {
                    message: message(AgentId::Muse, context, &failure),
                    parks_login: parks,
                    retry,
                },
                "{failure:?} in {which:?}"
            );
        }
    }
}

#[test]
fn a_rate_limit_holds_the_next_read_until_its_reset_and_never_past_a_day() {
    for (reset, floor) in [
        (RateLimitReset::RetryAfter(30), 30),
        (RateLimitReset::RetryAfter(10_000_000), 86_400),
        (RateLimitReset::At(NOW + 90), 90),
        (RateLimitReset::At(NOW - 90), 0),
        (RateLimitReset::At(0), 0),
        (RateLimitReset::Unknown, 0),
    ] {
        let failure = ReadFailure::RateLimited(reset);
        let floor = Duration::from_secs(floor);
        assert_eq!(failure.retry_floor(NOW), floor, "{reset:?}");
        let native = NativeOutcome {
            failure: Some(failure.clone()),
            login: None,
        };
        assert_eq!(native.retry_floor(NOW), floor, "{reset:?}");
        assert!(native.counts_toward_backoff(), "{reset:?}");
        assert_eq!(
            saved(AgentId::Codex, Saved::Home, &failure, NOW).retry,
            floor
        );
    }
    assert_eq!(NativeOutcome::default().retry_floor(NOW), Duration::ZERO);
    assert!(!NativeOutcome::default().counts_toward_backoff());
}

#[test]
fn a_login_is_verified_by_an_answer_for_its_own_account() {
    assert_eq!(verified(AgentId::Muse, None), Ok(()));
    for answered in [Unsupported::BillingIncomplete, Unsupported::Inactive] {
        assert_eq!(
            verified(
                AgentId::Muse,
                Some(&ReadFailure::Unsupported(answered.clone()))
            ),
            Ok(()),
            "{answered:?}"
        );
    }
    for failure in failures()
        .into_iter()
        .filter(|failure| *failure != ReadFailure::Unsupported(Unsupported::Inactive))
        .chain([
            ReadFailure::Unsupported(Unsupported::ApiKey),
            ReadFailure::Unsupported(Unsupported::AccountType("team".into())),
            ReadFailure::Unsupported(Unsupported::NoLimits),
        ])
    {
        assert_eq!(
            verified(AgentId::Muse, Some(&failure)),
            Err(message(AgentId::Muse, Context::Verify, &failure)),
            "{failure:?}"
        );
    }
}

#[test]
fn the_wording_names_each_providers_own_client_cli_and_vendor() {
    let words =
        |provider: AgentId, failure: ReadFailure| message(provider, Context::SignedIn, &failure);
    for (provider, install, limited, request, no_usage) in [
        (
            AgentId::Claude,
            "Install Claude Code and sign in with `claude` to see subscription limits.",
            "Anthropic is limiting how often Claude usage can be read. on-n-off will read it again later.",
            "The Claude usage read failed (HTTP 503). on-n-off will read it again later.",
            "Claude Code reported no usage.",
        ),
        (
            AgentId::Codex,
            "Install Codex CLI and sign in with `codex` to see subscription limits.",
            "OpenAI is limiting how often Codex usage can be read. on-n-off will read it again later.",
            "The Codex usage read failed (HTTP 503). on-n-off will read it again later.",
            "Codex CLI reported no usage.",
        ),
        (
            AgentId::Muse,
            "Install Muse Code and sign in with `muse` to see subscription limits.",
            "Meta is limiting how often Muse usage can be read. on-n-off will read it again later.",
            "The Muse usage read failed (HTTP 503). on-n-off will read it again later.",
            "Muse Code reported no usage.",
        ),
    ] {
        assert_eq!(words(provider, ReadFailure::NotInstalled), install);
        assert_eq!(
            words(provider, ReadFailure::RateLimited(RateLimitReset::Unknown)),
            limited
        );
        assert_eq!(
            words(provider, ReadFailure::RequestFailed("HTTP 503".into())),
            request
        );
        assert_eq!(words(provider, ReadFailure::NoUsage), no_usage);
    }
    assert_eq!(
        words(AgentId::Claude, ReadFailure::AccountChanged),
        "The signed-in Claude account changed while its usage was read."
    );
    assert_eq!(
        words(
            AgentId::Codex,
            ReadFailure::Unsupported(Unsupported::ApiKey)
        ),
        "Codex is signed in with an API key, which has no subscription limits."
    );
    assert_eq!(
        words(
            AgentId::Codex,
            ReadFailure::Unsupported(Unsupported::AccountType("team".into()))
        ),
        "Codex account type `team` has no subscription limits to show."
    );
    assert_eq!(
        words(
            AgentId::Muse,
            ReadFailure::Unsupported(Unsupported::BillingIncomplete)
        ),
        "Muse subscription billing is incomplete; finish it in Muse to see subscription limits."
    );
    assert_eq!(
        words(
            AgentId::Cursor,
            ReadFailure::Unsupported(Unsupported::NoLimits)
        ),
        "Cursor has no subscription limits to show."
    );
}

#[test]
fn an_http_failure_is_a_refused_login_a_rate_limit_or_a_failed_request_with_its_cause() {
    assert_eq!(
        ReadFailure::from(HttpError::Unauthorized),
        ReadFailure::LoginRefused
    );
    assert_eq!(
        ReadFailure::from(HttpError::RateLimited(RateLimitReset::RetryAfter(7))),
        ReadFailure::RateLimited(RateLimitReset::RetryAfter(7))
    );
    for (error, cause) in [
        (HttpError::Status(503), "HTTP 503"),
        (
            HttpError::Network("offline".into()),
            "network error: offline",
        ),
        (
            HttpError::Parse("not JSON".into()),
            "unreadable response: not JSON",
        ),
    ] {
        assert_eq!(
            ReadFailure::from(error),
            ReadFailure::RequestFailed(cause.to_string())
        );
    }
}

fn identity() -> Identity {
    Identity {
        provider: AgentId::Muse,
        user_id: "you@example.com".into(),
        workspace_id: "you@example.com".into(),
    }
}

fn weekly(used: f64) -> Reading {
    Reading {
        windows: vec![window(
            "weekly",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            used,
            None,
        )],
        ..Reading::default()
    }
}

fn plan_only() -> Reading {
    Reading {
        plan: Some("Wonder".into()),
        ..Reading::default()
    }
}

fn answered(id: &str, reading: Reading) -> Parsed {
    Parsed {
        account: Some(LimitsAccountDto {
            legacy_id: None,
            id: id.to_string(),
            label: Some("you@example.com".into()),
        }),
        reading,
    }
}

#[test]
fn an_answer_is_accepted_once_it_observed_something_for_the_expected_account() {
    let key = identity().observation_key();

    let kept = first_usage(&identity(), Ok(answered(&key, weekly(42.0))))
        .expect("accepted")
        .into_card();
    assert!(!kept.current_account);
    assert_eq!(kept.status, LimitsStatus::Ok);
    assert_eq!(
        first_usage(
            &identity(),
            Ok(answered("profile:someone-else", weekly(42.0)))
        ),
        None
    );
    assert_eq!(
        first_usage(&identity(), Ok(answered(&key, plan_only()))),
        None
    );

    let card = saved_card(
        &identity(),
        answered("whatever-the-reader-named", weekly(42.0)),
    )
    .expect("a saved answer is the profile's own card")
    .into_card();
    assert_eq!(
        card.account.as_ref().map(|account| account.id.as_str()),
        Some(key.as_str())
    );
    assert!(!card.current_account);
    assert_eq!(
        saved_card(&identity(), answered(&key, plan_only())),
        Err(ReadFailure::NoUsage)
    );

    let read = |reading| Read {
        account: None,
        answer: Ok(answered(&key, reading)),
    };
    let (shown, outcome) = signed_in(AgentId::Muse, read(plan_only()));
    assert_eq!(shown.status, LimitsStatus::Failed);
    assert_eq!(
        shown.message.as_deref(),
        Some("Muse Code reported no usage.")
    );
    assert_eq!(outcome.failure, Some(ReadFailure::NoUsage));
    let (shown, outcome) = signed_in(AgentId::Muse, read(weekly(42.0)));
    assert_eq!(shown.status, LimitsStatus::Ok);
    assert_eq!(outcome, NativeOutcome::default());
    assert!(shown.current_account);
}

#[test]
fn an_answer_that_observed_nothing_keeps_the_account_and_name_it_carried() {
    let key = identity().observation_key();
    let (card, _) = signed_in(
        AgentId::Codex,
        Read {
            account: Some(LimitsAccountDto {
                legacy_id: None,
                id: key.clone(),
                label: None,
            }),
            answer: Ok(answered(&key, plan_only())),
        },
    );

    let account = card.account.expect("the account the answer named");
    assert_eq!(account.id, key);
    assert_eq!(account.label.as_deref(), Some("you@example.com"));
}
