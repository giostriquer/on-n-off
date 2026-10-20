use std::path::PathBuf;

use super::{Cache, CachedRead, Changed};
use crate::accounts::{Forgot, Roster};
use crate::dto::{AgentId, LimitsAccountDto, ProviderLimitsDto};

pub(crate) trait AccountStores {
    fn forget_account(&self, account: &str, ids: &[String]) -> Result<Forgot, String>;
    fn forget_history(
        &self,
        account: &str,
        replaced: &[LimitsAccountDto],
    ) -> Result<Vec<String>, String>;
    fn set_archived(&self, ids: &[String], archived: bool) -> Result<bool, String>;
    fn roster(&self) -> Option<Roster>;
    fn read_again(&self) -> Changed;
}

pub(crate) struct AtHome {
    pub(crate) home: PathBuf,
    pub(crate) agent: AgentId,
}

impl AccountStores for AtHome {
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
    fn read_again(&self) -> Changed {
        super::read_served_quietly(self.agent, true).2
    }
}

struct CardHistory {
    account: String,
    replaced: Vec<LimitsAccountDto>,
}

impl CardHistory {
    fn planned(
        cache: &Cache,
        entries: &[ProviderLimitsDto],
        account: &str,
        signed_in: Option<&str>,
    ) -> Result<Self, String> {
        if account.trim().is_empty() {
            return Err("No account named.".into());
        }
        if let Some(refusal) = signed_in {
            if entries
                .iter()
                .any(|card| card.current_account && names(card, account))
            {
                return Err(refusal.into());
            }
        }
        let replaced = cache
            .column(entries)
            .into_iter()
            .find(|entry| !entry.card.current_account && names(&entry.card, account))
            .map(|entry| entry.replaced)
            .unwrap_or_default();
        Ok(Self {
            account: account.to_owned(),
            replaced,
        })
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
    let history = {
        let cached = cache.lock_read();
        CardHistory::planned(
            cache,
            entries_of(cached.as_ref()),
            account,
            Some("The signed-in account cannot be removed."),
        )?
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
    let relisted = forgot.changed && super::relisted(cache, None, || stores.roster());
    bumped(
        cache,
        Changed {
            limits: dropped || relisted,
            accounts: forgot.changed,
        },
    )
    .send(cache.source);
    match forgotten {
        Ok(_) => Ok(()),
        Err(error) if forgot.profile => Err(format!(
            "The saved login was removed, but its usage history was not: {error}"
        )),
        Err(error) => Err(error),
    }
}

pub(super) fn archive_with(
    cache: &Cache,
    account: &str,
    archived: bool,
    stores: &dyn AccountStores,
) -> Result<(), String> {
    let refusal = archived.then_some("The signed-in account cannot be archived.");
    archive_ids(cache, archived, stores, |entries| {
        CardHistory::planned(cache, entries, account, refusal).map(|history| history.ids())
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
    let relisted = changed && super::relisted(cache, None, || stores.roster());
    let edited = bumped(
        cache,
        Changed {
            limits: flagged || relisted,
            accounts: changed,
        },
    );
    let read = if archived {
        Changed::default()
    } else {
        stores.read_again()
    };
    edited.and(read).send(cache.source);
    Ok(())
}

fn entries_of(cached: Option<&CachedRead>) -> &[ProviderLimitsDto] {
    cached.map_or(&[], |read| &read.entries)
}

fn bumped(cache: &Cache, edited: Changed) -> Changed {
    if edited.limits {
        cache.revision.bump();
    }
    edited
}
