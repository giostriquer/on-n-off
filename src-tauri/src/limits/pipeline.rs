//! Provider-neutral credential, request, and status normalization.

use chrono::{SecondsFormat, Utc};

use super::credentials::CredentialLookup;
use super::http::HttpError;
use super::Parsed;
use crate::dto::{AgentId, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto};

/// Map login state and one provider load into the stable Limits DTO status model.
pub(super) fn resolve<T>(
    provider: AgentId,
    account: Option<LimitsAccountDto>,
    lookup: CredentialLookup<T>,
    load: impl FnOnce(&T) -> Result<Parsed, HttpError>,
) -> ProviderLimitsDto {
    let cli = provider.binary_name();
    let named = || Parsed {
        account: account.clone(),
        ..Parsed::default()
    };
    let credential = match lookup {
        CredentialLookup::Found(credential) => credential,
        CredentialLookup::Missing => {
            return finish(
                provider,
                LimitsStatus::SignedOut,
                Some(format!("Sign in with `{cli}` to see subscription limits.")),
                named(),
            );
        }
        CredentialLookup::Expired { renewable } => {
            return finish(
                provider,
                LimitsStatus::Unauthenticated,
                Some(token_expired(cli, renewable)),
                named(),
            );
        }
        CredentialLookup::Unsupported(why) => {
            return finish(provider, LimitsStatus::Unsupported, Some(why), named());
        }
        CredentialLookup::Unreadable(why) => {
            return finish(
                provider,
                LimitsStatus::Failed,
                Some(format!("Could not read the stored login: {why}")),
                named(),
            );
        }
    };
    match load(&credential) {
        Ok(mut parsed) => {
            parsed.windows.sort_by_key(|window| kind_rank(window.kind));
            finish(provider, LimitsStatus::Ok, None, parsed)
        }
        Err(HttpError::Unauthorized) => finish(
            provider,
            LimitsStatus::Unauthenticated,
            Some(relogin(cli)),
            named(),
        ),
        Err(error) => finish(
            provider,
            LimitsStatus::Failed,
            Some(format!(
                "Could not reach the {} usage service ({error}).",
                provider.display_name()
            )),
            named(),
        ),
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
