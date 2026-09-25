//! Provider-neutral credential, request, and status normalization.

use chrono::{SecondsFormat, Utc};

use super::credentials::CredentialLookup;
use super::Parsed;
use crate::dto::{AgentId, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto};
use crate::http::HttpError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ProviderLoadError {
    Http(HttpError),
    AccountMismatch,
}

impl From<HttpError> for ProviderLoadError {
    fn from(error: HttpError) -> Self {
        Self::Http(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LoadFailureKind {
    Unauthorized,
    AccountMismatch,
    Provider,
}

pub(super) struct ResolveOutcome {
    pub(super) dto: ProviderLimitsDto,
    pub(super) failure: Option<LoadFailureKind>,
}

/// Map login state and one provider load into the stable Limits DTO status model.
#[cfg(test)]
pub(super) fn resolve<T>(
    provider: AgentId,
    account: Option<LimitsAccountDto>,
    lookup: CredentialLookup<T>,
    load: impl FnOnce(&T) -> Result<Parsed, HttpError>,
) -> ProviderLimitsDto {
    resolve_provider(provider, account, lookup, |credential| {
        load(credential).map_err(ProviderLoadError::Http)
    })
    .dto
}

pub(super) fn resolve_provider<T>(
    provider: AgentId,
    account: Option<LimitsAccountDto>,
    lookup: CredentialLookup<T>,
    load: impl FnOnce(&T) -> Result<Parsed, ProviderLoadError>,
) -> ResolveOutcome {
    let cli = provider.binary_name();
    let named = || Parsed {
        account: account.clone(),
        ..Parsed::default()
    };
    let credential = match lookup {
        CredentialLookup::Found(credential) => credential,
        other => {
            let (status, message) = describe(cli, other);
            return ResolveOutcome {
                dto: finish(provider, status, Some(message), named()),
                failure: None,
            };
        }
    };
    match load(&credential) {
        Ok(parsed) => ResolveOutcome {
            dto: finish(provider, LimitsStatus::Ok, None, parsed),
            failure: None,
        },
        Err(ProviderLoadError::Http(HttpError::Unauthorized)) => ResolveOutcome {
            dto: finish(
                provider,
                LimitsStatus::Unauthenticated,
                Some(rejected(cli)),
                named(),
            ),
            failure: Some(LoadFailureKind::Unauthorized),
        },
        Err(ProviderLoadError::AccountMismatch) => ResolveOutcome {
            dto: finish(
                provider,
                LimitsStatus::Failed,
                Some(format!(
                    "The stored {cli} login belongs to a different account than the selected {cli} account. Run `{cli}`, select the intended account, send a prompt, then refresh here."
                )),
                named(),
            ),
            failure: Some(LoadFailureKind::AccountMismatch),
        },
        Err(ProviderLoadError::Http(error)) => ResolveOutcome {
            dto: finish(
                provider,
                LimitsStatus::Failed,
                Some(format!(
                    "Could not reach the {} usage service ({error}).",
                    provider.display_name()
                )),
                named(),
            ),
            failure: Some(LoadFailureKind::Provider),
        },
    }
}

pub(super) fn finish(
    provider: AgentId,
    status: LimitsStatus,
    message: Option<String>,
    mut parsed: Parsed,
) -> ProviderLimitsDto {
    // Native and saved reads share the same card priority, regardless of endpoint order.
    parsed.windows.sort_by_key(|window| kind_rank(window.kind));
    let observed_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    for window in &mut parsed.windows {
        if window.observed_at.is_empty() {
            window.observed_at.clone_from(&observed_at);
        }
    }
    ProviderLimitsDto {
        provider,
        status,
        message,
        account: parsed.account,
        current_account: true,
        plan: parsed.plan,
        subscription_status: parsed.subscription_status,
        windows: parsed.windows,
        credits: parsed.credits,
        workspace_credits: parsed.workspace_credits,
        credits_spent: parsed.credits_spent,
        subscription: parsed.subscription,
        reset_credits: parsed.reset_credits,
        reset_offer: parsed.reset_offer,
    }
}

fn relogin(cli: &str) -> String {
    format!("Login expired — run `{cli}` and sign in again to refresh subscription limits.")
}

/// The token the endpoint refused. Deliberately not "expired": a refusal says nothing about the
/// clock, and claiming otherwise sends a signed-in user to re-authenticate over a token the
/// provider simply stopped accepting.
fn rejected(cli: &str) -> String {
    format!(
        "The stored `{cli}` login was rejected — run `{cli}` and sign in again to refresh subscription limits."
    )
}

/// What a login state that is not a usable credential means for the user. One place, so a new
/// state is a line here rather than another ten-line arm in `resolve_provider`.
fn describe<T>(cli: &str, lookup: CredentialLookup<T>) -> (LimitsStatus, String) {
    match lookup {
        // `resolve_provider` takes this branch itself; a credential is not a failure to describe.
        CredentialLookup::Found(_) => unreachable!("a found credential is loaded, not described"),
        CredentialLookup::Missing => (
            LimitsStatus::SignedOut,
            format!("Sign in with `{cli}` to see subscription limits."),
        ),
        CredentialLookup::Expired { renewable } => {
            (LimitsStatus::Unauthenticated, token_expired(cli, renewable))
        }
        CredentialLookup::Unreadable(why) => (
            LimitsStatus::Failed,
            format!("Could not read the stored login: {why}"),
        ),
        CredentialLookup::Stranded(why) => (LimitsStatus::Unauthenticated, stranded(cli, &why)),
    }
}

/// on-n-off renewed the login and then could not store it, so the refresh token it spent is gone
/// and the CLI cannot renew itself either. Say what happened, and why, rather than name a remedy
/// that sounds ordinary: this is on-n-off's doing, and only a new sign-in clears it.
fn stranded(cli: &str, why: &str) -> String {
    format!(
        "on-n-off renewed the `{cli}` login but could not store it ({why}), so the stored login no longer works. Run `{cli}` and sign in again."
    )
}

fn token_expired(cli: &str, renewable: bool) -> String {
    if renewable {
        format!("Access token expired — send a prompt with `{cli}` to renew it, then refresh here.")
    } else {
        relogin(cli)
    }
}

pub(super) fn kind_rank(kind: LimitWindowKind) -> u8 {
    match kind {
        LimitWindowKind::Weekly => 0,
        LimitWindowKind::Session => 1,
        LimitWindowKind::Model => 2,
    }
}
