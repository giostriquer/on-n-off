use super::{
    model::Identity,
    store::{Database, Profile, Store},
    AccountsDto, ProfileDto,
};
use crate::dto::AgentId;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) type SignInDeadlines = BTreeMap<String, String>;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Roster {
    profiles: Vec<ProfileDto>,
    categories: BTreeMap<String, String>,
}

impl Roster {
    fn of(
        db: &Database,
        provider: AgentId,
        current: Option<&Identity>,
        archived: &BTreeSet<String>,
    ) -> Self {
        let categories = db
            .categories()
            .get(provider.key())
            .cloned()
            .unwrap_or_default();
        let profiles = db
            .profiles
            .iter()
            .filter(|p| p.identity.provider == provider)
            .map(|p| listed(p.clone(), current, archived, &categories, None))
            .collect();
        Self {
            profiles,
            categories,
        }
    }

    pub(super) fn read(home: &Path, provider: AgentId, current: Option<&Identity>) -> Option<Self> {
        if !Store::vault_exists(home) {
            return Some(Self::default());
        }
        let db = Store::open_unlocked(home)
            .and_then(|store| store.load())
            .ok()?;
        Some(Self::of(
            &db,
            provider,
            current,
            &crate::limits::archived(home, provider),
        ))
    }

    pub(crate) fn profiles(&self) -> &[ProfileDto] {
        &self.profiles
    }

    pub(crate) fn profile(&self, account_id: &str, ends: &SignInDeadlines) -> Option<ProfileDto> {
        self.profiles
            .iter()
            .find(|p| p.observation_id == account_id)
            .map(|p| p.ending(ends))
    }

    pub(crate) fn category(&self, account_id: &str) -> Option<&str> {
        self.categories.get(account_id).map(String::as_str)
    }

    pub(crate) fn set_category(&mut self, account_id: &str, category: Option<&str>) -> bool {
        let category = category.map(str::to_owned);
        let mut changed = match &category {
            Some(text) => self.categories.insert(account_id.to_owned(), text.clone()) != category,
            None => self.categories.remove(account_id).is_some(),
        };
        for profile in self
            .profiles
            .iter_mut()
            .filter(|p| p.observation_id == account_id)
        {
            changed |= profile.category != category;
            profile.category.clone_from(&category);
        }
        changed
    }

    pub(crate) fn set_archived(&mut self, ids: &[String], archived: bool) -> bool {
        let mut changed = false;
        for profile in self
            .profiles
            .iter_mut()
            .filter(|p| ids.contains(&p.observation_id))
        {
            changed |= profile.archived != archived;
            profile.archived = archived;
        }
        changed
    }
}

impl ProfileDto {
    pub(crate) fn observation_id(&self) -> &str {
        &self.observation_id
    }

    pub(crate) fn legacy_id(&self) -> Option<&str> {
        self.legacy_id.as_deref()
    }

    pub(crate) fn email(&self) -> Option<&str> {
        self.email.as_deref()
    }

    pub(crate) fn provider(&self) -> AgentId {
        self.identity.provider
    }

    pub(crate) fn active(&self) -> bool {
        self.active
    }

    pub(crate) fn archived(&self) -> bool {
        self.archived
    }

    fn ending(&self, ends: &SignInDeadlines) -> Self {
        Self {
            sign_in_ends_at: ends.get(&self.observation_id).cloned(),
            ..self.clone()
        }
    }
}

impl AccountsDto {
    pub(crate) fn sign_in_deadlines(&self) -> SignInDeadlines {
        self.profiles
            .iter()
            .filter_map(|p| Some((p.observation_id.clone(), p.sign_in_ends_at.clone()?)))
            .collect()
    }
}

pub(super) fn listed(
    p: Profile,
    current: Option<&Identity>,
    archived: &BTreeSet<String>,
    categories: &BTreeMap<String, String>,
    sign_in_ends_at: Option<String>,
) -> ProfileDto {
    let observation_id = p.identity.observation_key();
    ProfileDto {
        sign_in_ends_at,
        needs_login: p.needs_login(),
        archived: archived.contains(&observation_id),
        category: categories.get(&observation_id).cloned(),
        legacy_id: p.identity.legacy_key(),
        active: current == Some(&p.identity),
        observation_id,
        id: p.id,
        identity: p.identity,
        label: p.label,
        email: p.email,
        saved_at: p.saved_at,
        pending_activation: p.pending_activation,
    }
}

#[cfg(test)]
pub(crate) fn roster_for_test(
    provider: AgentId,
    profiles: &[(Identity, &str)],
    current: Option<&Identity>,
    archived: &[String],
    categories: &[(&str, &str)],
) -> Roster {
    let mut db = Database::default();
    for (index, (identity, email)) in profiles.iter().enumerate() {
        db.profiles.push(Profile {
            id: format!("saved-{index}"),
            identity: identity.clone(),
            label: (*email).to_string(),
            email: Some((*email).to_string()),
            category: None,
            saved_at: "2026-09-13T12:00:00Z".into(),
            login: None,
            pending_activation: false,
            usage_renewal_owned: false,
            home: Some(format!("home-{index}")),
            signed_out: false,
        });
    }
    for (account_id, category) in categories {
        db.set_account_category(provider, account_id, category)
            .expect("a category the vault accepts");
    }
    Roster::of(&db, provider, current, &archived.iter().cloned().collect())
}
