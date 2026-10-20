use std::path::Path;

use super::pipeline::finish;
use super::Parsed;
use crate::accounts::model::AccessToken;
use crate::accounts::muse_store::{MuseCredential, MuseLogin};
use crate::cli_locate::resolve_provider_cli;
use crate::dto::{AgentId, LimitsAccountDto, LimitsStatus, ProviderLimitsDto};
use crate::http::HttpError;

pub(super) mod mint;

#[derive(Debug, PartialEq, Eq)]
enum ReadFailure {
    NotInstalled,
    SignedOut,
    Unsupported(String),
    Failed(LimitsAccountDto, String),
}

fn account_dto(label: Option<String>) -> LimitsAccountDto {
    match label
        .as_deref()
        .and_then(crate::accounts::muse_store::identity)
    {
        Some(identity) => super::scoped_account(&identity, label),
        None => LimitsAccountDto {
            legacy_id: None,
            id: super::DEFAULT_ACCOUNT.to_string(),
            label,
        },
    }
}

fn finish_read(read: Result<Parsed, ReadFailure>) -> ProviderLimitsDto {
    match read {
        Ok(parsed) => finish(AgentId::Muse, LimitsStatus::Ok, None, parsed),
        Err(ReadFailure::NotInstalled) => finish(
            AgentId::Muse,
            LimitsStatus::SignedOut,
            Some(
                "Install Muse Code and sign in with `muse` to see subscription limits.".to_string(),
            ),
            Parsed::default(),
        ),
        Err(ReadFailure::SignedOut) => finish(
            AgentId::Muse,
            LimitsStatus::SignedOut,
            Some("Sign in with `muse` to see subscription limits.".to_string()),
            Parsed::default(),
        ),
        Err(ReadFailure::Unsupported(message)) => finish(
            AgentId::Muse,
            LimitsStatus::Unsupported,
            Some(message),
            Parsed::default(),
        ),
        Err(ReadFailure::Failed(account, message)) => finish(
            AgentId::Muse,
            LimitsStatus::Failed,
            Some(message),
            Parsed {
                account: Some(account),
                ..Parsed::default()
            },
        ),
    }
}

pub(super) fn muse_current(home: &Path) -> ProviderLimitsDto {
    finish_read(read_current(
        home,
        || resolve_provider_cli(AgentId::Muse, "muse").is_some(),
        crate::accounts::muse_store::signed_in,
        |token| mint::fetch(mint::KEY_ENDPOINT, token),
    ))
}

fn read_current(
    home: &Path,
    installed: impl FnOnce() -> bool,
    signed_in: impl FnOnce(&Path) -> Option<MuseLogin>,
    mint: impl FnOnce(&AccessToken) -> Result<mint::MintResponse, HttpError>,
) -> Result<Parsed, ReadFailure> {
    let Some(MuseLogin { email, credential }) = signed_in(&crate::paths::muse_config_for(home))
    else {
        return Err(if installed() {
            ReadFailure::SignedOut
        } else {
            ReadFailure::NotInstalled
        });
    };
    let why = match credential {
        MuseCredential::Token(token) => return mint::current_read(email, mint(&token)),
        MuseCredential::ApiKey => {
            return Err(ReadFailure::Unsupported(
                "Muse is signed in with an API key, which has no subscription limits.".to_string(),
            ))
        }
        MuseCredential::Expired => {
            "Muse's login has expired. Sign in with `muse` again to see subscription limits."
        }
        MuseCredential::Unread => {
            "on-n-off could not read Muse's login from the Keychain, so it did not ask Meta for usage. It reads the login again when Muse's login changes or after on-n-off restarts."
        }
    };
    Err(ReadFailure::Failed(account_dto(email), why.to_string()))
}

#[cfg(test)]
mod tests;
