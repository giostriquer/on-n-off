use super::snapshots::{superseded, supersedes};
use crate::accounts::{Listed, Roster, SignInDeadlines};
use crate::dto::{
    ColumnEntryDto, LimitsAccountDto, LimitsStatus, ProviderLimitsDto, ReadStanding, Reading,
};

pub(crate) fn column(
    entries: &[ProviderLimitsDto],
    roster: &Roster,
    ends: &SignInDeadlines,
    now_ms: i64,
) -> Vec<ColumnEntryDto> {
    let signed_in = entries
        .iter()
        .find(|card| card.current_account)
        .and_then(|card| card.account.as_ref())
        .map(|account| account.id.as_str());
    let cards: Vec<ProviderLimitsDto> = entries
        .iter()
        .cloned()
        .map(|card| with_legacy_id(card, roster))
        .collect();
    let hidden = superseded(&cards);
    let mut column: Vec<ColumnEntryDto> = cards
        .iter()
        .zip(&hidden)
        .filter(|(_, hidden)| !**hidden)
        .map(|(card, _)| {
            let id = card.account.as_ref().map(|account| account.id.as_str());
            ColumnEntryDto {
                standing: ReadStanding::sent_with(card),
                profile: id
                    .and_then(|id| roster.profile(id))
                    .map(|profile| roster.in_column(profile, ends, signed_in, now_ms)),
                category: id.and_then(|id| roster.category(id)).map(str::to_owned),
                replaced: cards
                    .iter()
                    .filter(|legacy| supersedes(card, legacy))
                    .filter_map(|legacy| legacy.account.clone())
                    .collect(),
                unread: false,
                card: card.clone(),
            }
        })
        .collect();
    for profile in roster.profiles() {
        let read = cards.iter().any(|card| {
            card.account
                .as_ref()
                .is_some_and(|account| account.id == profile.observation_id())
        });
        if !read {
            column.push(unread(profile, roster, ends, now_ms));
        }
    }
    column
}

fn with_legacy_id(mut card: ProviderLimitsDto, roster: &Roster) -> ProviderLimitsDto {
    if let Some(account) = card.account.as_mut() {
        if account.legacy_id.is_none() {
            account.legacy_id = roster
                .profile(&account.id)
                .and_then(Listed::legacy_id)
                .map(str::to_owned);
        }
    }
    card
}

fn unread(
    profile: &Listed,
    roster: &Roster,
    ends: &SignInDeadlines,
    now_ms: i64,
) -> ColumnEntryDto {
    let id = profile.observation_id();
    let card = ProviderLimitsDto {
        provider: profile.provider(),
        status: LimitsStatus::Ok,
        message: None,
        account: Some(LimitsAccountDto {
            id: id.to_owned(),
            legacy_id: profile.legacy_id().map(str::to_owned),
            label: profile.email().map(str::to_owned),
        }),
        current_account: false,
        saved_profile: false,
        archived: profile.archived(),
        reading: Reading::default(),
    };
    ColumnEntryDto {
        standing: ReadStanding::sent_with(&card),
        card,
        profile: Some(roster.in_column(profile, ends, None, now_ms)),
        category: roster.category(id).map(str::to_owned),
        replaced: Vec::new(),
        unread: true,
    }
}

#[cfg(test)]
mod tests;
