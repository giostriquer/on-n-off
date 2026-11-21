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
    muse_current_with(home, |token| mint::fetch(mint::KEY_ENDPOINT, token))
}

fn muse_current_with(
    home: &Path,
    mint: impl Fn(&AccessToken) -> Result<mint::MintResponse, HttpError>,
) -> Read {
    let config = crate::paths::muse_config_for(home);
    read_again_if_told(|| {
        read_recording_refusal(
            home,
            || resolve_provider_cli(AgentId::Muse, "muse").is_some(),
            crate::accounts::muse_store::signed_in,
            &mint,
            |token| crate::accounts::muse_store::record_refusal(&config, token),
        )
    })
}

fn read_again_if_told(mut read: impl FnMut() -> (Read, bool)) -> Read {
    let (first, again) = read();
    if again {
        read().0
    } else {
        first
    }
}

fn read_recording_refusal(
    home: &Path,
    installed: impl FnOnce() -> bool,
    signed_in: impl FnOnce(&Path) -> Option<MuseLogin>,
    mint: impl FnOnce(&AccessToken) -> Result<mint::MintResponse, HttpError>,
    record_refusal: impl FnOnce(&AccessToken) -> bool,
) -> (Read, bool) {
    let mut read_again = false;
    let read = read_current(home, installed, signed_in, |token| {
        let answer = mint(token);
        if matches!(answer, Err(HttpError::Unauthorized)) {
            read_again = record_refusal(token);
        }
        answer
    });
    (read, read_again)
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
