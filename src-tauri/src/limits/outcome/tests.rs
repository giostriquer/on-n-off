use super::*;
use crate::dto::LimitWindowKind;
use crate::limits::json::window;
use Context::{AccountHome, SavedLogin, SignedIn};

const NOW: i64 = 1_800_000_000;
const OK: LimitsStatus = LimitsStatus::Ok;
const FAILED: LimitsStatus = LimitsStatus::Failed;
const SIGNED_OUT: LimitsStatus = LimitsStatus::SignedOut;
const UNSUPPORTED: LimitsStatus = LimitsStatus::Unsupported;

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
        ReadFailure::Unavailable("The source said why.".to_string()),
    ]
}

struct Row {
    failure: ReadFailure,
    context: Context,
    status: LimitsStatus,
    message: &'static str,
    keeps_account: bool,
    counted: bool,
    floor: u64,
    parks_login: bool,
}

#[allow(clippy::too_many_arguments)]
fn row(
    failure: ReadFailure,
    context: Context,
    status: LimitsStatus,
    message: &'static str,
    keeps_account: bool,
    counted: bool,
    floor: u64,
    parks_login: bool,
) -> Row {
    Row {
        failure,
        context,
        status,
        message,
        keeps_account,
        counted,
        floor,
        parks_login,
    }
}

#[test]
fn every_failure_in_every_read_context_has_one_card_and_one_schedule() {
    use ReadFailure as F;
    let limited = || F::RateLimited(RateLimitReset::RetryAfter(30));
    let why = || F::Unavailable("The source said why.".to_string());
    let inactive = || F::Unsupported(Unsupported::Inactive);
    const LIMITED: &str =
        "Meta is limiting how often Muse usage can be read. on-n-off will read it again later.";
    const INACTIVE: &str =
        "This Muse subscription is inactive, so it has no subscription limits to show.";
    const VAULT_ENDED: &str =
        "Usage refresh needs sign-in again or renewal by the client that owns this login.";
    const HOME_ENDED: &str = "This account's saved login has ended. Sign in again.";
    const OTHER: &str = "This saved login now signs in as a different account. Sign in again.";
    let rows = [
        row(F::NotInstalled, SignedIn, SIGNED_OUT, "Install Muse Code and sign in with `muse` to see subscription limits.", false, false, 0, false),
        row(F::SignedOut, SignedIn, SIGNED_OUT, "Sign in with `muse` to see subscription limits.", false, false, 0, false),
        row(inactive(), SignedIn, UNSUPPORTED, INACTIVE, true, false, 0, false),
        row(F::LoginRefused, SignedIn, FAILED, "Muse's login has ended. Sign in with `muse` again to see subscription limits.", true, true, 0, false),
        row(F::OtherAccount, SignedIn, FAILED, "Muse answered for a different account. Sign in with `muse` again to see subscription limits.", true, true, 0, false),
        row(F::AccountChanged, SignedIn, FAILED, "The signed-in Muse account changed while its usage was read.", true, true, 0, false),
        row(limited(), SignedIn, FAILED, LIMITED, true, true, 30, false),
        row(why(), SignedIn, FAILED, "The source said why.", true, true, 0, false),
        row(F::NotInstalled, SavedLogin, FAILED, "Muse Code is not installed.", true, true, 0, false),
        row(F::SignedOut, SavedLogin, FAILED, VAULT_ENDED, true, true, 0, true),
        row(inactive(), SavedLogin, FAILED, INACTIVE, true, true, 0, false),
        row(F::LoginRefused, SavedLogin, FAILED, VAULT_ENDED, true, true, 0, true),
        row(F::OtherAccount, SavedLogin, FAILED, OTHER, true, true, 0, true),
        row(F::AccountChanged, SavedLogin, FAILED, "This account changed while its usage was read.", true, true, 0, false),
        row(limited(), SavedLogin, FAILED, LIMITED, true, true, 30, false),
        row(why(), SavedLogin, FAILED, "The source said why.", true, true, 0, false),
        row(F::NotInstalled, AccountHome, FAILED, "Muse Code is not installed.", true, true, 0, false),
        row(F::SignedOut, AccountHome, FAILED, HOME_ENDED, true, true, 0, false),
        row(inactive(), AccountHome, FAILED, INACTIVE, true, true, 0, false),
        row(F::LoginRefused, AccountHome, FAILED, HOME_ENDED, true, true, 0, false),
        row(F::OtherAccount, AccountHome, FAILED, OTHER, true, true, 0, false),
        row(F::AccountChanged, AccountHome, FAILED, "This account changed while its usage was read.", true, true, 0, false),
        row(limited(), AccountHome, FAILED, LIMITED, true, true, 30, false),
        row(why(), AccountHome, FAILED, "The source said why.", true, true, 0, false),
    ];
    assert_eq!(
        rows.len(),
        failures().len() * 3,
        "every failure in every context"
    );
    for row in rows {
        let case = format!("{:?} in {:?}", row.failure, row.context);
        assert_eq!(
            verdict(AgentId::Muse, row.context, &row.failure, NOW),
            Verdict {
                status: row.status,
                message: row.message.to_string(),
                keeps_account: row.keeps_account,
                backoff: Backoff {
                    counted: row.counted,
                    floor: Duration::from_secs(row.floor),
                },
                parks_login: row.parks_login,
            },
            "{case}"
        );
    }
}

#[test]
fn a_verification_and_a_first_usage_read_each_failure_their_own_way() {
    let display_failure =
        "The Muse login could not be verified. Check connectivity or sign in again.";
    for failure in failures() {
        let expected = match &failure {
            ReadFailure::Unsupported(Unsupported::Inactive) => Ok(()),
            ReadFailure::OtherAccount => {
                Err("Muse answered for a different account. Sign in again.".to_string())
            }
            _ => Err(display_failure.to_string()),
        };
        assert_eq!(
            verified(AgentId::Muse, &Err(failure.clone())),
            expected,
            "{failure:?}"
        );
        assert_eq!(
            first_usage(&identity(), Err(failure.clone())),
            None,
            "{failure:?}"
        );
    }
    assert_eq!(verified(AgentId::Muse, &Ok(())), Ok(()));
    for answered in [Unsupported::BillingIncomplete, Unsupported::Inactive] {
        assert_eq!(
            verified(
                AgentId::Muse,
                &Err(ReadFailure::Unsupported(answered.clone()))
            ),
            Ok(()),
            "{answered:?}"
        );
    }
    for other_login in [
        Unsupported::ApiKey,
        Unsupported::AccountType("team".into()),
        Unsupported::NoLimits,
    ] {
        assert!(
            verified(
                AgentId::Muse,
                &Err(ReadFailure::Unsupported(other_login.clone()))
            )
            .is_err(),
            "{other_login:?}"
        );
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
        for context in [SignedIn, SavedLogin, AccountHome] {
            let backoff = verdict(
                AgentId::Codex,
                context,
                &ReadFailure::RateLimited(reset),
                NOW,
            )
            .backoff;
            assert_eq!(
                backoff,
                Backoff {
                    counted: true,
                    floor: Duration::from_secs(floor)
                },
                "{reset:?} in {context:?}"
            );
        }
    }
}

#[test]
fn the_wording_names_each_providers_own_client_cli_and_vendor() {
    let words = |provider: AgentId, failure: ReadFailure| {
        verdict(provider, SignedIn, &failure, NOW).message
    };
    for (provider, install, sign_in, limited, api_key) in [
        (
            AgentId::Claude,
            "Install Claude Code and sign in with `claude` to see subscription limits.",
            "Sign in with `claude` to see subscription limits.",
            "Anthropic is limiting how often Claude usage can be read. on-n-off will read it again later.",
            "Claude is signed in with an API key, which has no subscription limits.",
        ),
        (
            AgentId::Codex,
            "Install Codex CLI and sign in with `codex` to see subscription limits.",
            "Sign in with `codex` to see subscription limits.",
            "OpenAI is limiting how often Codex usage can be read. on-n-off will read it again later.",
            "Codex is signed in with an API key, which has no subscription limits.",
        ),
        (
            AgentId::Muse,
            "Install Muse Code and sign in with `muse` to see subscription limits.",
            "Sign in with `muse` to see subscription limits.",
            "Meta is limiting how often Muse usage can be read. on-n-off will read it again later.",
            "Muse is signed in with an API key, which has no subscription limits.",
        ),
    ] {
        assert_eq!(words(provider, ReadFailure::NotInstalled), install);
        assert_eq!(words(provider, ReadFailure::SignedOut), sign_in);
        assert_eq!(
            words(provider, ReadFailure::RateLimited(RateLimitReset::Unknown)),
            limited
        );
        assert_eq!(
            words(provider, ReadFailure::Unsupported(Unsupported::ApiKey)),
            api_key
        );
    }
    assert_eq!(
        words(AgentId::Claude, ReadFailure::AccountChanged),
        "The signed-in Claude account changed while its usage was read."
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
fn an_http_failure_is_a_refused_login_a_rate_limit_or_unavailable_with_its_cause() {
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
            ReadFailure::Unavailable(format!(
                "The usage read failed ({cause}). on-n-off will read it again later."
            ))
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
    let no_usage = ReadFailure::Unavailable("Muse Code reported no usage.".into());
    let plan_only = Reading {
        plan: Some("Wonder".into()),
        ..Reading::default()
    };

    let kept = first_usage(&identity(), Ok(answered(&key, weekly(42.0)))).expect("accepted");
    assert!(!kept.current_account);
    assert_eq!(kept.status, OK);
    assert_eq!(
        first_usage(
            &identity(),
            Ok(answered("profile:someone-else", weekly(42.0)))
        ),
        None
    );
    assert_eq!(
        first_usage(&identity(), Ok(answered(&key, plan_only.clone()))),
        None
    );

    let card = saved_card(
        &identity(),
        answered("whatever-the-reader-named", weekly(42.0)),
    )
    .expect("a saved answer is the profile's own card");
    assert_eq!(
        card.account.as_ref().map(|a| a.id.as_str()),
        Some(key.as_str())
    );
    assert!(!card.current_account);
    assert_eq!(
        saved_card(&identity(), answered(&key, plan_only.clone())),
        Err(no_usage.clone())
    );

    assert_eq!(accepted(&identity(), card.clone()), Ok(card.clone()));
    let mut another = card.clone();
    another.account.as_mut().unwrap().id = "profile:someone-else".into();
    assert_eq!(
        accepted(&identity(), another),
        Err(ReadFailure::OtherAccount)
    );
    let mut empty = card;
    empty.reading = plan_only.clone();
    assert_eq!(accepted(&identity(), empty), Err(no_usage.clone()));

    let read = |reading| Read {
        account: None,
        answer: Ok(answered(&key, reading)),
    };
    let (shown, backoff) = signed_in_at(AgentId::Muse, read(plan_only), NOW);
    assert_eq!(
        (shown.status, shown.message.as_deref(), backoff.counted),
        (FAILED, Some("Muse Code reported no usage."), true)
    );
    let (shown, backoff) = signed_in_at(AgentId::Muse, read(weekly(42.0)), NOW);
    assert_eq!((shown.status, backoff), (OK, Backoff::default()));
    assert!(shown.current_account);
}

#[test]
fn a_signed_in_card_keeps_the_account_known_before_a_failure_unless_nobody_is_signed_in() {
    let known = answered("profile:you", Reading::default()).account;
    for failure in failures() {
        let (card, _) = signed_in_at(
            AgentId::Codex,
            Read {
                account: known.clone(),
                answer: Err(failure.clone()),
            },
            NOW,
        );
        let nobody = matches!(failure, ReadFailure::NotInstalled | ReadFailure::SignedOut);
        assert_eq!(card.account.is_none(), nobody, "{failure:?}");
        assert!(card.current_account, "{failure:?}");
        assert!(!card.reading.has_observations(), "{failure:?}");
    }
}
