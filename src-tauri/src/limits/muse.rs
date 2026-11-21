use std::path::Path;

use super::outcome::{Read, ReadFailure, Unsupported};
use crate::accounts::model::AccessToken;
use crate::accounts::muse_store::{MuseCredential, MuseLogin};
use crate::cli_locate::resolve_provider_cli;
use crate::dto::{AgentId, LimitsAccountDto};
use crate::http::HttpError;

pub(super) mod mint;

const UNREAD: &str = "on-n-off could not read Muse's login, so it did not ask Meta for usage. It reads the login again when you ask it to check limits, when Muse signs in to another account or after on-n-off restarts.";

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

pub(super) fn known_account(home: &Path) -> Option<LimitsAccountDto> {
    crate::accounts::muse_store::login_email(&crate::paths::muse_config_for(home)).map(account_dto)
}

pub(super) fn reread_login(home: &Path) -> bool {
    crate::accounts::muse_store::ask_again(&crate::paths::muse_config_for(home))
}

pub(super) fn muse_current(home: &Path) -> Read {
    let config = crate::paths::muse_config_for(home);
    once_more_if_refused(
        || {
            read_noting_refusal(
                home,
                || resolve_provider_cli(AgentId::Muse, "muse").is_some(),
                crate::accounts::muse_store::signed_in,
                |token| mint::fetch(mint::KEY_ENDPOINT, token),
            )
        },
        |token| crate::accounts::muse_store::record_refusal(&config, token),
    )
}

fn once_more_if_refused(
    mut read: impl FnMut() -> (Read, Option<AccessToken>),
    mut record_refusal: impl FnMut(&AccessToken) -> bool,
) -> Read {
    let (first, refused) = read();
    if !refused.is_some_and(|token| record_refusal(&token)) {
        return first;
    }
    let (again, refused) = read();
    if let Some(token) = refused {
        record_refusal(&token);
    }
    again
}

fn read_noting_refusal(
    home: &Path,
    installed: impl FnOnce() -> bool,
    signed_in: impl FnOnce(&Path) -> Option<MuseLogin>,
    mint: impl FnOnce(&AccessToken) -> Result<mint::MintResponse, HttpError>,
) -> (Read, Option<AccessToken>) {
    let mut refused = None;
    let read = read_current(home, installed, signed_in, |token| {
        let answer = mint(token);
        if matches!(answer, Err(HttpError::Unauthorized)) {
            refused = Some(token.clone());
        }
        answer
    });
    (read, refused)
}

fn read_current(
    home: &Path,
    installed: impl FnOnce() -> bool,
    signed_in: impl FnOnce(&Path) -> Option<MuseLogin>,
    mint: impl FnOnce(&AccessToken) -> Result<mint::MintResponse, HttpError>,
) -> Read {
    let Some(MuseLogin { email, credential }) = signed_in(&crate::paths::muse_config_for(home))
    else {
        return Read::anonymous(if installed() {
            ReadFailure::SignedOut
        } else {
            ReadFailure::NotInstalled
        });
    };
    let account = Some(account_dto(email.clone()));
    let answer = match credential {
        MuseCredential::Token(token) => mint(&token)
            .map_err(ReadFailure::from)
            .and_then(|response| mint::answer(email, &response)),
        MuseCredential::ApiKey => Err(ReadFailure::Unsupported(Unsupported::ApiKey)),
        MuseCredential::Expired => Err(ReadFailure::LoginRefused),
        MuseCredential::Unread => Err(ReadFailure::Unavailable(UNREAD.to_string())),
    };
    Read { account, answer }
}

#[cfg(test)]
mod tests;
