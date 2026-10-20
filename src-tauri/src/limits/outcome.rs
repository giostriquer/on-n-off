use std::time::Duration;

use super::pipeline::finish;
use super::{scoped_account, Parsed};
use crate::accounts::model::Identity;
use crate::dto::{AgentId, LimitsAccountDto, LimitsStatus, ProviderLimitsDto, Reading};
use crate::http::{HttpError, RateLimitReset};
use crate::provider_catalogue::LimitsFacts;

const LONGEST_RETRY_FLOOR: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReadFailure {
    NotInstalled,
    SignedOut,
    Unsupported(Unsupported),
    LoginRefused,
    OtherAccount,
    AccountChanged,
    RateLimited(RateLimitReset),
    RequestFailed(String),
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Unsupported {
    NoLimits,
    ApiKey,
    AccountType(String),
    BillingIncomplete,
    Inactive,
}

impl From<HttpError> for ReadFailure {
    fn from(error: HttpError) -> Self {
        match error {
            HttpError::Unauthorized => Self::LoginRefused,
            HttpError::RateLimited(reset) => Self::RateLimited(reset),
            other => Self::RequestFailed(other.to_string()),
        }
    }
}

#[derive(Debug)]
pub(super) struct Read {
    pub(super) account: Option<LimitsAccountDto>,
    pub(super) answer: Result<Parsed, ReadFailure>,
}

impl Read {
    pub(super) fn anonymous(failure: ReadFailure) -> Self {
        Self {
            account: None,
            answer: Err(failure),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Context {
    SignedIn,
    SavedLogin,
    AccountHome,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Backoff {
    pub(crate) counted: bool,
    pub(crate) floor: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub(crate) status: LimitsStatus,
    pub(crate) message: String,
    pub(crate) keeps_account: bool,
    pub(crate) backoff: Backoff,
    pub(crate) parks_login: bool,
}

pub(crate) fn verdict(
    provider: AgentId,
    context: Context,
    failure: &ReadFailure,
    now_secs: i64,
) -> Verdict {
    let nobody = matches!(failure, ReadFailure::NotInstalled | ReadFailure::SignedOut);
    let status = match (context, failure) {
        (Context::SignedIn, _) if nobody => LimitsStatus::SignedOut,
        (Context::SignedIn, ReadFailure::Unsupported(_)) => LimitsStatus::Unsupported,
        _ => LimitsStatus::Failed,
    };
    Verdict {
        message: Words::of(provider).message(context, failure),
        keeps_account: context != Context::SignedIn || !nobody,
        backoff: Backoff {
            counted: status == LimitsStatus::Failed,
            floor: retry_floor(failure, now_secs),
        },
        parks_login: context == Context::SavedLogin
            && matches!(
                failure,
                ReadFailure::SignedOut | ReadFailure::LoginRefused | ReadFailure::OtherAccount
            ),
        status,
    }
}

pub(super) fn nobody_signed_in(failure: &ReadFailure) -> bool {
    matches!(
        failure,
        ReadFailure::SignedOut | ReadFailure::Unsupported(_)
    )
}

pub(super) fn signed_in(provider: AgentId, read: Read) -> (ProviderLimitsDto, Backoff) {
    signed_in_at(provider, read, chrono::Utc::now().timestamp())
}

pub(super) fn signed_in_at(
    provider: AgentId,
    read: Read,
    now_secs: i64,
) -> (ProviderLimitsDto, Backoff) {
    let answer = read
        .answer
        .and_then(|parsed| accept(provider, None, parsed));
    match answer {
        Ok(parsed) => (
            finish(provider, LimitsStatus::Ok, None, parsed),
            Backoff::default(),
        ),
        Err(failure) => {
            let verdict = verdict(provider, Context::SignedIn, &failure, now_secs);
            let card = finish(
                provider,
                verdict.status,
                Some(verdict.message),
                Parsed {
                    account: read.account.filter(|_| verdict.keeps_account),
                    reading: Reading::default(),
                },
            );
            (card, verdict.backoff)
        }
    }
}

pub(super) fn saved_card(
    identity: &Identity,
    mut parsed: Parsed,
) -> Result<ProviderLimitsDto, ReadFailure> {
    let label = parsed.account.and_then(|account| account.label);
    parsed.account = Some(scoped_account(identity, label));
    card_for(identity, parsed)
}

pub(super) fn first_usage(
    identity: &Identity,
    answer: Result<Parsed, ReadFailure>,
) -> Option<ProviderLimitsDto> {
    answer.and_then(|parsed| card_for(identity, parsed)).ok()
}

fn card_for(identity: &Identity, parsed: Parsed) -> Result<ProviderLimitsDto, ReadFailure> {
    let parsed = accept(identity.provider, Some(&identity.observation_key()), parsed)?;
    let mut card = finish(identity.provider, LimitsStatus::Ok, None, parsed);
    card.current_account = false;
    Ok(card)
}

pub(crate) fn accepted(
    identity: &Identity,
    card: ProviderLimitsDto,
) -> Result<ProviderLimitsDto, ReadFailure> {
    answers_for(
        identity.provider,
        Some(&identity.observation_key()),
        card.account.as_ref(),
        &card.reading,
    )?;
    Ok(card)
}

pub(crate) fn verified(provider: AgentId, answer: &Result<(), ReadFailure>) -> Result<(), String> {
    let display = provider.display_name();
    match answer {
        Ok(())
        | Err(ReadFailure::Unsupported(Unsupported::Inactive | Unsupported::BillingIncomplete)) => {
            Ok(())
        }
        Err(ReadFailure::OtherAccount) => Err(format!(
            "{display} answered for a different account. Sign in again."
        )),
        Err(_) => Err(format!(
            "The {display} login could not be verified. Check connectivity or sign in again."
        )),
    }
}

fn accept(
    provider: AgentId,
    expected: Option<&str>,
    parsed: Parsed,
) -> Result<Parsed, ReadFailure> {
    answers_for(provider, expected, parsed.account.as_ref(), &parsed.reading)?;
    Ok(parsed)
}

fn answers_for(
    provider: AgentId,
    expected: Option<&str>,
    account: Option<&LimitsAccountDto>,
    reading: &Reading,
) -> Result<(), ReadFailure> {
    if expected.is_some_and(|expected| account.map(|account| account.id.as_str()) != Some(expected))
    {
        return Err(ReadFailure::OtherAccount);
    }
    if !reading.has_observations() {
        return Err(no_usage(provider));
    }
    Ok(())
}

pub(super) fn no_usage(provider: AgentId) -> ReadFailure {
    ReadFailure::Unavailable(format!("{} reported no usage.", Words::of(provider).client))
}

fn retry_floor(failure: &ReadFailure, now_secs: i64) -> Duration {
    let seconds = match failure {
        ReadFailure::RateLimited(RateLimitReset::RetryAfter(seconds)) => *seconds,
        ReadFailure::RateLimited(RateLimitReset::At(at)) => {
            u64::try_from(at.saturating_sub(now_secs)).unwrap_or(0)
        }
        _ => 0,
    };
    Duration::from_secs(seconds).min(LONGEST_RETRY_FLOOR)
}

struct Words {
    display: &'static str,
    cli: &'static str,
    client: &'static str,
    vendor: &'static str,
}

impl Words {
    fn of(provider: AgentId) -> Self {
        let facts = provider.facts();
        let LimitsFacts { client, vendor } = facts.limits.unwrap_or(LimitsFacts {
            client: facts.display_name,
            vendor: facts.display_name,
        });
        Self {
            display: facts.display_name,
            cli: facts.binary,
            client,
            vendor,
        }
    }

    fn message(&self, context: Context, failure: &ReadFailure) -> String {
        let Self {
            display,
            cli,
            client,
            vendor,
        } = self;
        let saved = context != Context::SignedIn;
        match failure {
            ReadFailure::NotInstalled if saved => format!("{client} is not installed."),
            ReadFailure::NotInstalled => {
                format!("Install {client} and sign in with `{cli}` to see subscription limits.")
            }
            ReadFailure::SignedOut | ReadFailure::LoginRefused
                if context == Context::SavedLogin =>
            {
                "Usage refresh needs sign-in again or renewal by the client that owns this login."
                    .to_string()
            }
            ReadFailure::SignedOut | ReadFailure::LoginRefused => {
                if saved {
                    "This account's saved login has ended. Sign in again.".to_string()
                } else if *failure == ReadFailure::SignedOut {
                    format!("Sign in with `{cli}` to see subscription limits.")
                } else {
                    format!(
                        "{display}'s login has ended. Sign in with `{cli}` again to see subscription limits."
                    )
                }
            }
            ReadFailure::OtherAccount if saved => {
                "This saved login now signs in as a different account. Sign in again.".to_string()
            }
            ReadFailure::OtherAccount => format!(
                "{display} answered for a different account. Sign in with `{cli}` again to see subscription limits."
            ),
            ReadFailure::AccountChanged if saved => {
                "This account changed while its usage was read.".to_string()
            }
            ReadFailure::AccountChanged => {
                format!("The signed-in {display} account changed while its usage was read.")
            }
            ReadFailure::RateLimited(_) => format!(
                "{vendor} is limiting how often {display} usage can be read. on-n-off will read it again later."
            ),
            ReadFailure::Unsupported(why) => match why {
                Unsupported::NoLimits => format!("{display} has no subscription limits to show."),
                Unsupported::ApiKey => format!(
                    "{display} is signed in with an API key, which has no subscription limits."
                ),
                Unsupported::AccountType(kind) => {
                    format!("{display} account type `{kind}` has no subscription limits to show.")
                }
                Unsupported::BillingIncomplete => format!(
                    "{display} subscription billing is incomplete; finish it in {display} to see subscription limits."
                ),
                Unsupported::Inactive => format!(
                    "This {display} subscription is inactive, so it has no subscription limits to show."
                ),
            },
            ReadFailure::RequestFailed(cause) => format!(
                "The {display} usage read failed ({cause}). on-n-off will read it again later."
            ),
            ReadFailure::Unavailable(why) => why.clone(),
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
