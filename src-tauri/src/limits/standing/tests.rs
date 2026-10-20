use super::cases::fixture;
use super::*;

#[test]
fn every_card_stands_as_its_row_says_in_the_shape_the_ui_reads() {
    for case in fixture().cases {
        assert_eq!(
            serde_json::to_value(ReadStanding::of(&case.card())).unwrap(),
            case.standing,
            "{}",
            case.name
        );
    }
}

#[test]
fn the_limits_column_sends_each_card_as_it_is_with_its_standing_left_out_while_live() {
    use crate::accounts::{Roster, SignInDeadlines};
    for case in fixture().cases {
        let card = case.card();
        let column = crate::limits::column(
            std::slice::from_ref(&card),
            &Roster::default(),
            &SignInDeadlines::new(),
            0,
        );
        let [entry] = column.as_slice() else {
            panic!("{}: one entry for one card", case.name);
        };
        let mut sent = serde_json::to_value(entry).unwrap();
        let standing = sent.as_object_mut().unwrap().remove("standing");
        assert_eq!(
            standing,
            (case.expected() != ReadStanding::Live).then(|| case.standing.clone()),
            "{}",
            case.name
        );
        assert_eq!(sent, serde_json::to_value(&card).unwrap(), "{}", case.name);
    }
}

#[test]
fn a_signed_in_read_that_did_not_answer_stands_unavailable_in_the_read_outcomes_words() {
    use super::super::outcome::{self, Context, Read, ReadFailure, Unsupported};
    use crate::dto::{AgentId, MessageTone};
    use crate::http::RateLimitReset;
    let variant_and_tone = |failure: &ReadFailure| match failure {
        ReadFailure::NotInstalled => (0, MessageTone::Muted),
        ReadFailure::SignedOut => (1, MessageTone::Muted),
        ReadFailure::Unsupported(_) => (2, MessageTone::Muted),
        ReadFailure::LoginRefused => (3, MessageTone::Error),
        ReadFailure::OtherAccount => (4, MessageTone::Error),
        ReadFailure::AccountChanged => (5, MessageTone::Error),
        ReadFailure::RateLimited(_) => (6, MessageTone::Error),
        ReadFailure::RequestFailed(_) => (7, MessageTone::Error),
        ReadFailure::NoUsage => (8, MessageTone::Error),
        ReadFailure::Unavailable(_) => (9, MessageTone::Error),
    };
    let failures = [
        ReadFailure::NotInstalled,
        ReadFailure::SignedOut,
        ReadFailure::Unsupported(Unsupported::ApiKey),
        ReadFailure::LoginRefused,
        ReadFailure::OtherAccount,
        ReadFailure::AccountChanged,
        ReadFailure::RateLimited(RateLimitReset::RetryAfter(30)),
        ReadFailure::RequestFailed("timed out".into()),
        ReadFailure::NoUsage,
        ReadFailure::Unavailable("Could not read the stored login.".into()),
    ];
    assert_eq!(
        failures
            .iter()
            .map(|failure| variant_and_tone(failure).0)
            .collect::<std::collections::BTreeSet<_>>(),
        (0..10).collect(),
        "one failure of every ReadFailure variant"
    );
    for failure in failures {
        let (card, _) = outcome::signed_in(AgentId::Codex, Read::anonymous(failure.clone()));
        assert_eq!(
            ReadStanding::of(&card),
            ReadStanding::Unavailable {
                message: StandingMessage {
                    text: outcome::message(AgentId::Codex, Context::SignedIn, &failure),
                    tone: variant_and_tone(&failure).1,
                },
            },
            "{failure:?}"
        );
    }
}

#[test]
fn a_card_that_did_not_answer_without_words_says_its_provider_did_not_answer() {
    use crate::dto::{AgentId, LimitsStatus, MessageTone};
    let mut failed = ProviderLimitsDto::for_test(AgentId::Codex, "acct");
    failed.status = LimitsStatus::Failed;
    let mut signed_out = ProviderLimitsDto::for_test(AgentId::Claude, "acct");
    signed_out.status = LimitsStatus::SignedOut;
    let unavailable = |text: &str, tone| ReadStanding::Unavailable {
        message: StandingMessage {
            text: text.into(),
            tone,
        },
    };
    assert_eq!(
        ReadStanding::of(&failed),
        unavailable("Codex did not answer.", MessageTone::Error)
    );
    assert_eq!(
        ReadStanding::of(&signed_out),
        unavailable("Claude did not answer.", MessageTone::Muted)
    );
}
