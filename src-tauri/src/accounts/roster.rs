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
    pub(super) fn of(
        db: &Database,
        provider: AgentId,
        current: Option<&Identity>,
        archived: &BTreeSet<String>,
        ends: &dyn Fn(&Profile) -> Option<String>,
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
            .map(|p| listed(p.clone(), current, archived, &categories, ends(p)))
            .collect();
        Self {
            profiles,
            categories,
        }
    }

    pub(crate) fn read(home: &Path, provider: AgentId) -> Option<Self> {
        if !Store::vault_exists(home) {
            return Some(Self::default());
        }
        let db = Store::open_unlocked(home)
            .and_then(|store| store.load())
            .ok()?;
        Some(Self::of(
            &db,
            provider,
            None,
            &crate::limits::archived(home, provider),
            &|_| None,
        ))
    }

    pub(super) fn into_profiles(self) -> Vec<ProfileDto> {
        self.profiles
    }

    pub(crate) fn profiles(&self) -> &[ProfileDto] {
        &self.profiles
    }

    pub(crate) fn profile(&self, account_id: &str) -> Option<&ProfileDto> {
        self.profiles
            .iter()
            .find(|p| p.observation_id == account_id)
    }

    pub(crate) fn category(&self, account_id: &str) -> Option<&str> {
        self.categories.get(account_id).map(String::as_str)
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

    pub(crate) fn archived(&self) -> bool {
        self.archived
    }

    #[cfg(test)]
    pub(crate) fn active(&self) -> bool {
        self.active
    }

    pub(crate) fn in_column(&self, ends: &SignInDeadlines, signed_in: Option<&str>) -> Self {
        Self {
            sign_in_ends_at: ends.get(&self.observation_id).cloned(),
            active: signed_in == Some(self.observation_id.as_str()),
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

fn listed(
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
    Roster::of(
        &db,
        provider,
        None,
        &archived.iter().cloned().collect(),
        &|_| None,
    )
}
