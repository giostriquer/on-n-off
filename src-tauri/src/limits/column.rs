use super::snapshots::{superseded, supersedes};
use crate::accounts::{ProfileDto, Roster, SignInDeadlines};
use crate::dto::{ColumnEntryDto, LimitsAccountDto, LimitsStatus, ProviderLimitsDto, Reading};

pub(crate) fn column(
    entries: &[ProviderLimitsDto],
    roster: &Roster,
    ends: &SignInDeadlines,
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
                profile: id
                    .and_then(|id| roster.profile(id))
                    .map(|profile| profile.in_column(ends, signed_in)),
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
            column.push(unread(profile, roster, ends));
        }
    }
    column
}

fn with_legacy_id(mut card: ProviderLimitsDto, roster: &Roster) -> ProviderLimitsDto {
    if let Some(account) = card.account.as_mut() {
        if account.legacy_id.is_none() {
            account.legacy_id = roster
                .profile(&account.id)
                .and_then(ProfileDto::legacy_id)
                .map(str::to_owned);
        }
    }
    card
}

fn unread(profile: &ProfileDto, roster: &Roster, ends: &SignInDeadlines) -> ColumnEntryDto {
    let id = profile.observation_id();
    ColumnEntryDto {
        card: ProviderLimitsDto {
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
        },
        profile: Some(profile.in_column(ends, None)),
        category: roster.category(id).map(str::to_owned),
        replaced: Vec::new(),
        unread: true,
    }
}

#[cfg(test)]
mod tests;
