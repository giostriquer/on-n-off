use super::{
    login_state::{Deadline, LoginState, Standing},
    store::{Database, Profile, Store},
    AccountsDto, ProfileDto,
};
use crate::dto::AgentId;
use crate::limits::outcome::Parked;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) type SignInDeadlines = BTreeMap<String, String>;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Roster {
    profiles: Vec<Listed>,
    categories: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Listed {
    profile: ProfileDto,
    ends_at_ms: Option<i64>,
    parked: Option<Parked>,
}

pub(super) struct Facts {
    pub(super) active: bool,
    pub(super) deadline: Deadline,
    pub(super) parked: Option<Parked>,
}

impl Roster {
    pub(super) fn of(
        db: &Database,
        provider: AgentId,
        archived: &BTreeSet<String>,
        now_ms: i64,
        facts: &dyn Fn(&Profile) -> Facts,
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
            .map(|p| listed(p, &facts(p), archived, &categories, now_ms))
            .collect();
        Self {
            profiles,
            categories,
        }
    }

    pub(super) fn recorded(db: &Database, home: &Path, provider: AgentId) -> Self {
        Self::of(
            db,
            provider,
            &crate::limits::archived(home, provider),
            chrono::Utc::now().timestamp_millis(),
            &|p| Facts {
                active: false,
                deadline: Deadline::recorded(p),
                parked: super::usage::recorded_parked(home, p),
            },
        )
    }

    pub(crate) fn read(home: &Path, provider: AgentId) -> Option<Self> {
        if !Store::vault_exists(home) {
            return Some(Self::default());
        }
        let db = Store::open_unlocked(home)
            .and_then(|store| store.load())
            .ok()?;
        Some(Self::recorded(&db, home, provider))
    }

    pub(super) fn into_profiles(self) -> Vec<ProfileDto> {
        self.profiles.into_iter().map(|p| p.profile).collect()
    }

    pub(crate) fn profiles(&self) -> &[Listed] {
        &self.profiles
    }

    pub(crate) fn profile(&self, account_id: &str) -> Option<&Listed> {
        self.profiles
            .iter()
            .find(|p| p.profile.observation_id == account_id)
    }

    pub(crate) fn category(&self, account_id: &str) -> Option<&str> {
        self.categories.get(account_id).map(String::as_str)
    }
}

impl Listed {
    pub(crate) fn observation_id(&self) -> &str {
        &self.profile.observation_id
    }

    pub(crate) fn legacy_id(&self) -> Option<&str> {
        self.profile.legacy_id.as_deref()
    }

    pub(crate) fn email(&self) -> Option<&str> {
        self.profile.email.as_deref()
    }

    pub(crate) fn provider(&self) -> AgentId {
        self.profile.identity.provider
    }

    pub(crate) fn archived(&self) -> bool {
        self.profile.archived
    }

    pub(crate) fn in_column(
        &self,
        ends: &SignInDeadlines,
        signed_in: Option<&str>,
        now_ms: i64,
    ) -> ProfileDto {
        let active = signed_in == Some(self.observation_id());
        let listed_end = ends
            .get(self.observation_id())
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.timestamp_millis());
        let deadline = Deadline::at(listed_end.or(self.ends_at_ms));
        let standing = if active {
            Standing::SignedIn
        } else {
            Standing::Saved(self.parked)
        };
        ProfileDto {
            active,
            sign_in_ends_at: deadline.spelled(),
            login_state: LoginState::of(self.profile.needs_login, &deadline, standing, now_ms),
            ..self.profile.clone()
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
    p: &Profile,
    facts: &Facts,
    archived: &BTreeSet<String>,
    categories: &BTreeMap<String, String>,
    now_ms: i64,
) -> Listed {
    let observation_id = p.identity.observation_key();
    let standing = if facts.active {
        Standing::SignedIn
    } else {
        Standing::Saved(facts.parked)
    };
    Listed {
        profile: ProfileDto {
            sign_in_ends_at: facts.deadline.spelled(),
            login_state: LoginState::of(p.needs_login(), &facts.deadline, standing, now_ms),
            needs_login: p.needs_login(),
            archived: archived.contains(&observation_id),
            category: categories.get(&observation_id).cloned(),
            legacy_id: p.identity.legacy_key(),
            active: facts.active,
            observation_id,
            id: p.id.clone(),
            identity: p.identity.clone(),
            label: p.label.clone(),
            email: p.email.clone(),
            saved_at: p.saved_at.clone(),
            pending_activation: p.pending_activation,
        },
        ends_at_ms: facts.deadline.ends_at_ms(),
        parked: facts.parked,
    }
}

#[cfg(test)]
pub(crate) fn roster_for_test(
    provider: AgentId,
    profiles: &[(super::model::Identity, &str)],
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
            sign_in_end: super::login_state::SignInEnd::Unread,
        });
    }
    for (account_id, category) in categories {
        db.set_account_category(provider, account_id, category)
            .expect("a category the vault accepts");
    }
    Roster::of(
        &db,
        provider,
        &archived.iter().cloned().collect(),
        0,
        &|_| Facts {
            active: false,
            deadline: Deadline::at(None),
            parked: None,
        },
    )
}
