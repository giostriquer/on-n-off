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
    NoUsage,
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

impl ReadFailure {
    fn names_nobody(&self) -> bool {
        matches!(self, Self::NotInstalled | Self::SignedOut)
    }

    pub(crate) fn retry_floor(&self, now_secs: i64) -> Duration {
        let seconds = match self {
            Self::RateLimited(RateLimitReset::RetryAfter(seconds)) => *seconds,
            Self::RateLimited(RateLimitReset::At(at)) => {
                u64::try_from(at.saturating_sub(now_secs)).unwrap_or(0)
            }
            _ => 0,
        };
        Duration::from_secs(seconds).min(LONGEST_RETRY_FLOOR)
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
pub(crate) enum Saved {
    Login,
    Home,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Context {
    SignedIn,
    Saved(Saved),
    Verify,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NativeOutcome {
    pub(crate) failure: Option<ReadFailure>,
    pub(crate) login: Option<String>,
}

impl NativeOutcome {
    pub(crate) fn counts_toward_backoff(&self) -> bool {
        self.failure
            .as_ref()
            .is_some_and(|failure| signed_in_status(failure) == LimitsStatus::Failed)
    }

    pub(crate) fn retry_floor(&self, now_secs: i64) -> Duration {
        self.failure
            .as_ref()
            .map_or(Duration::ZERO, |failure| failure.retry_floor(now_secs))
    }
}

pub(super) fn signed_in(provider: AgentId, read: Read) -> (ProviderLimitsDto, NativeOutcome) {
    let login = read.account.as_ref().map(|account| account.id.clone());
    let (failure, account) = match read.answer {
        Ok(parsed) if parsed.reading.has_observations() => {
            let card = finish(provider, LimitsStatus::Ok, None, parsed);
            return (
                card,
                NativeOutcome {
                    failure: None,
                    login,
                },
            );
        }
        Ok(parsed) => (ReadFailure::NoUsage, parsed.account),
        Err(failure) => {
            let account = read.account.filter(|_| !failure.names_nobody());
            (failure, account)
        }
    };
    let card = finish(
        provider,
        signed_in_status(&failure),
        Some(message(provider, Context::SignedIn, &failure)),
        Parsed {
            account,
            reading: Reading::default(),
        },
    );
    (
        card,
        NativeOutcome {
            failure: Some(failure),
            login,
        },
    )
}

fn signed_in_status(failure: &ReadFailure) -> LimitsStatus {
    match failure {
        ReadFailure::NotInstalled | ReadFailure::SignedOut => LimitsStatus::SignedOut,
        ReadFailure::Unsupported(_) => LimitsStatus::Unsupported,
        _ => LimitsStatus::Failed,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SavedVerdict {
    pub(crate) message: String,
    pub(crate) parks_login: bool,
    pub(crate) retry: Duration,
}

pub(crate) fn saved(
    provider: AgentId,
    saved: Saved,
    failure: &ReadFailure,
    now_secs: i64,
) -> SavedVerdict {
    SavedVerdict {
        message: message(provider, Context::Saved(saved), failure),
        parks_login: saved == Saved::Login
            && matches!(
                failure,
                ReadFailure::SignedOut | ReadFailure::LoginRefused | ReadFailure::OtherAccount
            ),
        retry: failure.retry_floor(now_secs),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Accepted(ProviderLimitsDto);

impl Accepted {
    pub(crate) fn into_card(self) -> ProviderLimitsDto {
        self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(card: ProviderLimitsDto) -> Self {
        Self(card)
    }
}

pub(super) fn saved_card(identity: &Identity, mut parsed: Parsed) -> Result<Accepted, ReadFailure> {
    let label = parsed.account.and_then(|account| account.label);
    parsed.account = Some(scoped_account(identity, label));
    card_for(identity, parsed)
}

pub(super) fn first_usage(
    identity: &Identity,
    answer: Result<Parsed, ReadFailure>,
) -> Option<Accepted> {
    answer.and_then(|parsed| card_for(identity, parsed)).ok()
}

fn card_for(identity: &Identity, parsed: Parsed) -> Result<Accepted, ReadFailure> {
    if parsed.account.as_ref().map(|account| account.id.as_str())
        != Some(identity.observation_key().as_str())
    {
        return Err(ReadFailure::OtherAccount);
    }
    if !parsed.reading.has_observations() {
        return Err(ReadFailure::NoUsage);
    }
    let mut card = finish(identity.provider, LimitsStatus::Ok, None, parsed);
    card.current_account = false;
    Ok(Accepted(card))
}

pub(crate) fn verified(provider: AgentId, failure: Option<&ReadFailure>) -> Result<(), String> {
    match failure {
        None
        | Some(ReadFailure::Unsupported(Unsupported::Inactive | Unsupported::BillingIncomplete)) => {
            Ok(())
        }
        Some(failure) => Err(message(provider, Context::Verify, failure)),
    }
}

pub(crate) fn message(provider: AgentId, context: Context, failure: &ReadFailure) -> String {
    let facts = provider.facts();
    let LimitsFacts { client, vendor } = facts.limits.unwrap_or(LimitsFacts {
        client: facts.display_name,
        vendor: facts.display_name,
    });
    let (display, cli) = (facts.display_name, facts.binary);
    match (context, failure) {
        (Context::Verify, ReadFailure::OtherAccount) => {
            format!("{display} answered for a different account. Sign in again.")
        }
        (Context::Verify, _) => format!(
            "The {display} login could not be verified. Check connectivity or sign in again."
        ),
        (Context::SignedIn, ReadFailure::NotInstalled) => {
            format!("Install {client} and sign in with `{cli}` to see subscription limits.")
        }
        (Context::Saved(_), ReadFailure::NotInstalled) => format!("{client} is not installed."),
        (Context::SignedIn, ReadFailure::SignedOut) => {
            format!("Sign in with `{cli}` to see subscription limits.")
        }
        (Context::SignedIn, ReadFailure::LoginRefused) => format!(
            "{display}'s login has ended. Sign in with `{cli}` again to see subscription limits."
        ),
        (Context::Saved(Saved::Login), ReadFailure::SignedOut | ReadFailure::LoginRefused) => {
            "Usage refresh needs sign-in again or renewal by the client that owns this login."
                .to_string()
        }
        (Context::Saved(Saved::Home), ReadFailure::SignedOut | ReadFailure::LoginRefused) => {
            "This account's saved login has ended. Sign in again.".to_string()
        }
        (Context::SignedIn, ReadFailure::OtherAccount) => format!(
            "{display} answered for a different account. Sign in with `{cli}` again to see subscription limits."
        ),
        (Context::Saved(_), ReadFailure::OtherAccount) => {
            "This saved login now signs in as a different account. Sign in again.".to_string()
        }
        (Context::SignedIn, ReadFailure::AccountChanged) => {
            format!("The signed-in {display} account changed while its usage was read.")
        }
        (Context::Saved(_), ReadFailure::AccountChanged) => {
            "This account changed while its usage was read.".to_string()
        }
        (_, ReadFailure::RateLimited(_)) => format!(
            "{vendor} is limiting how often {display} usage can be read. on-n-off will read it again later."
        ),
        (_, ReadFailure::RequestFailed(cause)) => format!(
            "The {display} usage read failed ({cause}). on-n-off will read it again later."
        ),
        (_, ReadFailure::NoUsage) => format!("{client} reported no usage."),
        (_, ReadFailure::Unsupported(Unsupported::NoLimits)) => {
            format!("{display} has no subscription limits to show.")
        }
        (_, ReadFailure::Unsupported(Unsupported::ApiKey)) => {
            format!("{display} is signed in with an API key, which has no subscription limits.")
        }
        (_, ReadFailure::Unsupported(Unsupported::AccountType(kind))) => {
            format!("{display} account type `{kind}` has no subscription limits to show.")
        }
        (_, ReadFailure::Unsupported(Unsupported::BillingIncomplete)) => format!(
            "{display} subscription billing is incomplete; finish it in {display} to see subscription limits."
        ),
        (_, ReadFailure::Unsupported(Unsupported::Inactive)) => format!(
            "This {display} subscription is inactive, so it has no subscription limits to show."
        ),
        (_, ReadFailure::Unavailable(why)) => why.clone(),
    }
}

#[cfg(test)]
pub(super) mod tests;
