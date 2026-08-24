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
        CredentialLookup::Missing => {
            return ResolveOutcome {
                dto: finish(
                    provider,
                    LimitsStatus::SignedOut,
                    Some(format!("Sign in with `{cli}` to see subscription limits.")),
                    named(),
                ),
                failure: None,
            };
        }
        CredentialLookup::Expired { renewable } => {
            return ResolveOutcome {
                dto: finish(
                    provider,
                    LimitsStatus::Unauthenticated,
                    Some(token_expired(cli, renewable)),
                    named(),
                ),
                failure: None,
            };
        }
        CredentialLookup::Unreadable(why) => {
            return ResolveOutcome {
                dto: finish(
                    provider,
                    LimitsStatus::Failed,
                    Some(format!("Could not read the stored login: {why}")),
                    named(),
                ),
                failure: None,
            };
        }
    };
    match load(&credential) {
        Ok(mut parsed) => {
            parsed.windows.sort_by_key(|window| kind_rank(window.kind));
            ResolveOutcome {
                dto: finish(provider, LimitsStatus::Ok, None, parsed),
                failure: None,
            }
        }
        Err(ProviderLoadError::Http(HttpError::Unauthorized)) => ResolveOutcome {
            dto: finish(
                provider,
                LimitsStatus::Unauthenticated,
                Some(relogin(cli)),
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
        windows: parsed.windows,
        credits: parsed.credits,
    }
}

fn relogin(cli: &str) -> String {
    format!("Login expired — run `{cli}` and sign in again to refresh subscription limits.")
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
