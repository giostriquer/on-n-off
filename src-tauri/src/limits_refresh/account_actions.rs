use std::path::PathBuf;

use super::{Cache, CachedRead, Changed};
use crate::accounts::{Forgot, Roster};
use crate::dto::{AgentId, LimitsAccountDto, ProviderLimitsDto};

pub(crate) trait AccountStores {
    fn check_history(&self, replaced: &[LimitsAccountDto]) -> Result<(), String>;
    fn forget_account(&self, account: &str, ids: &[String]) -> Result<Forgot, String>;
    fn forget_history(
        &self,
        account: &str,
        replaced: &[LimitsAccountDto],
    ) -> Result<Vec<String>, String>;
    fn set_archived(&self, ids: &[String], archived: bool) -> Result<bool, String>;
    fn roster(&self) -> Option<Roster>;
    fn read_again(&self, force: bool) -> Changed;
}

pub(crate) struct AtHome {
    pub(crate) home: PathBuf,
    pub(crate) agent: AgentId,
    pub(crate) read: fn(AgentId, bool) -> Changed,
}

impl AccountStores for AtHome {
    fn check_history(&self, replaced: &[LimitsAccountDto]) -> Result<(), String> {
        crate::limits::check_history(&self.home, self.agent, replaced)
    }
    fn forget_account(&self, account: &str, ids: &[String]) -> Result<Forgot, String> {
        crate::accounts::forget_account_at(&self.home, self.agent, account, ids)
    }
    fn forget_history(
        &self,
        account: &str,
        replaced: &[LimitsAccountDto],
    ) -> Result<Vec<String>, String> {
        crate::limits::forget_history(&self.home, self.agent, account, replaced)
    }
    fn set_archived(&self, ids: &[String], archived: bool) -> Result<bool, String> {
        crate::limits::set_archived_at(&self.home, self.agent, ids, archived)
    }
    fn roster(&self) -> Option<Roster> {
        Roster::read(&self.home, self.agent)
    }
    fn read_again(&self, force: bool) -> Changed {
        (self.read)(self.agent, force)
    }
}

struct CardHistory {
    account: String,
    replaced: Vec<LimitsAccountDto>,
}

impl CardHistory {
    fn planned(cache: &Cache, entries: &[ProviderLimitsDto], account: &str) -> Self {
        let replaced = cache
            .column(entries)
            .into_iter()
            .find(|entry| !entry.card.current_account && names(&entry.card, account))
            .map(|entry| entry.replaced)
            .unwrap_or_default();
        Self {
            account: account.to_owned(),
            replaced,
        }
    }

    fn ids(&self) -> Vec<String> {
        self.replaced
            .iter()
            .map(|legacy| legacy.id.clone())
            .chain([self.account.clone()])
            .collect()
    }
}

fn names(card: &ProviderLimitsDto, account: &str) -> bool {
    card.account
        .as_ref()
        .is_some_and(|named| named.id == account)
}

fn named(account: &str) -> Result<&str, String> {
    if account.trim().is_empty() {
        Err("No account named.".into())
    } else {
        Ok(account)
    }
}

fn refuse_signed_in(
    entries: &[ProviderLimitsDto],
    account: &str,
    refusal: &str,
) -> Result<(), String> {
    if entries
        .iter()
        .any(|card| card.current_account && names(card, account))
    {
        Err(refusal.into())
    } else {
        Ok(())
    }
}

fn drop_cards(entries: &mut Vec<ProviderLimitsDto>, ids: &[String]) -> bool {
    let before = entries.len();
    entries.retain(|card| {
        card.current_account
            || card
                .account
                .as_ref()
                .is_none_or(|account| !ids.contains(&account.id))
    });
    entries.len() != before
}

pub(super) fn remove_with(
    cache: &Cache,
    account: &str,
    stores: &dyn AccountStores,
) -> Result<(), String> {
    let account = named(account)?;
    let history = {
        let cached = cache.lock_read();
        let entries = entries_of(cached.as_ref());
        refuse_signed_in(entries, account, "The signed-in account cannot be removed.")?;
        let history = CardHistory::planned(cache, entries, account);
        stores.check_history(&history.replaced)?;
        history
    };
    let forgot = stores.forget_account(&history.account, &history.ids())?;
    let (forgotten, dropped) = {
        let mut cached = cache.lock_read();
        let forgotten = stores.forget_history(&history.account, &history.replaced);
        let dropped = match (&forgotten, cached.as_mut()) {
            (Ok(ids), Some(read)) => drop_cards(&mut read.entries, ids),
            _ => false,
        };
        (forgotten, dropped)
    };
    edited(cache, dropped, forgot.changed, &|| stores.roster()).send(cache.source);
    match forgotten {
        Ok(_) => Ok(()),
        Err(error) if forgot.profile => Err(match &forgot.home_left {
            Some(why) => format!(
                "The saved login was removed, but its usage history was not: {error} Its \
                 sign-in home goes at a later read: {why}"
            ),
            None => format!("The saved login was removed, but its usage history was not: {error}"),
        }),
        Err(error) => Err(error),
    }
}

pub(super) fn archive_with(
    cache: &Cache,
    account: &str,
    archived: bool,
    stores: &dyn AccountStores,
) -> Result<(), String> {
    let account = named(account)?;
    archive_ids(cache, archived, stores, |entries| {
        if archived {
            refuse_signed_in(
                entries,
                account,
                "The signed-in account cannot be archived.",
            )?;
        }
        Ok(CardHistory::planned(cache, entries, account).ids())
    })
}

pub(super) fn archive_ids(
    cache: &Cache,
    archived: bool,
    stores: &dyn AccountStores,
    ids: impl FnOnce(&[ProviderLimitsDto]) -> Result<Vec<String>, String>,
) -> Result<(), String> {
    let (flagged, changed) = {
        let mut cached = cache.lock_read();
        let ids = ids(entries_of(cached.as_ref()))?;
        if ids.is_empty() {
            return Err("No account to archive.".into());
        }
        let changed = stores.set_archived(&ids, archived)?;
        let flagged = cached.as_mut().is_some_and(|read| {
            crate::limits::mark_archived(&mut read.entries, |id, was| {
                if ids.iter().any(|named| named == id) {
                    archived
                } else {
                    was
                }
            })
        });
        (flagged, changed)
    };
    let edited = edited(cache, flagged, changed, &|| stores.roster());
    let announced = if archived {
        Changed::default()
    } else {
        stores.read_again(true)
    };
    edited.without(announced).send(cache.source);
    Ok(())
}

pub(super) fn forget_with(
    cache: &Cache,
    forget: impl FnOnce() -> Result<Vec<String>, String>,
) -> Result<Vec<String>, String> {
    let (forgotten, dropped) = {
        let mut cached = cache.lock_read();
        let forgotten = forget()?;
        let dropped = cached
            .as_mut()
            .is_some_and(|read| drop_cards(&mut read.entries, &forgotten));
        (forgotten, dropped)
    };
    edited(cache, dropped, false, &|| None).send(cache.source);
    Ok(forgotten)
}

fn entries_of(cached: Option<&CachedRead>) -> &[ProviderLimitsDto] {
    cached.map_or(&[], |read| &read.entries)
}

fn edited(
    cache: &Cache,
    cards: bool,
    stored: bool,
    roster: &dyn Fn() -> Option<Roster>,
) -> Changed {
    let relisted = stored && super::relisted(cache, None, roster);
    let changed = Changed {
        limits: cards || relisted,
        accounts: stored,
    };
    if changed.limits {
        cache.revision.bump();
    }
    changed
}
