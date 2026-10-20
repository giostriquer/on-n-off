use super::snapshots::supersedes;
use crate::accounts::{ProfileDto, Roster, SignInDeadlines};
use crate::dto::{ColumnEntryDto, LimitsAccountDto, LimitsStatus, ProviderLimitsDto, Reading};

pub(crate) fn column(
    entries: &[ProviderLimitsDto],
    roster: &Roster,
    ends: &SignInDeadlines,
) -> Vec<ColumnEntryDto> {
    let mut column: Vec<ColumnEntryDto> = entries
        .iter()
        .map(|card| with_account_facts(card.clone(), roster, ends))
        .collect();
    fold_replaced_history(&mut column);
    for profile in roster.profiles() {
        let read = column.iter().any(|entry| {
            entry
                .card
                .account
                .as_ref()
                .is_some_and(|account| account.id == profile.observation_id())
        });
        if !read {
            column.push(unread(profile, roster, ends));
        }
    }
    column
}

fn with_account_facts(
    mut card: ProviderLimitsDto,
    roster: &Roster,
    ends: &SignInDeadlines,
) -> ColumnEntryDto {
    let id = card.account.as_ref().map(|account| account.id.clone());
    let profile = id.as_deref().and_then(|id| roster.profile(id, ends));
    if let (Some(account), Some(profile)) = (card.account.as_mut(), &profile) {
        if account.legacy_id.is_none() {
            account.legacy_id = profile.legacy_id().map(str::to_owned);
        }
    }
    ColumnEntryDto {
        category: id
            .as_deref()
            .and_then(|id| roster.category(id))
            .map(str::to_owned),
        card,
        profile,
        replaced: Vec::new(),
        unread: false,
    }
}

fn fold_replaced_history(column: &mut Vec<ColumnEntryDto>) {
    let replacing: Vec<Vec<usize>> = column
        .iter()
        .map(|legacy| {
            column
                .iter()
                .enumerate()
                .filter(|(_, scoped)| supersedes(&scoped.card, &legacy.card))
                .map(|(index, _)| index)
                .collect()
        })
        .collect();
    for (legacy, scoped) in replacing.iter().enumerate() {
        let Some(account) = column[legacy].card.account.clone() else {
            continue;
        };
        for &index in scoped {
            column[index].replaced.push(LimitsAccountDto {
                legacy_id: None,
                ..account.clone()
            });
        }
    }
    let mut index = 0;
    column.retain(|_| {
        index += 1;
        replacing[index - 1].is_empty()
    });
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
            archived: profile.archived() && !profile.active(),
            reading: Reading::default(),
        },
        profile: roster.profile(id, ends),
        category: roster.category(id).map(str::to_owned),
        replaced: Vec::new(),
        unread: true,
    }
}

#[cfg(test)]
mod tests;
