use super::{
    login_state::{Deadline, LoginState, Standing},
    store::{Database, Profile, Store},
    usage::LoginKey,
    AccountsDto, ProfileDto,
};
use crate::dto::AgentId;
use crate::limits::outcome::Parked;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) type SignInDeadlines = BTreeMap<String, String>;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Roster {
    home: PathBuf,
    profiles: Vec<Listed>,
    categories: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Listed {
    profile: ProfileDto,
    ends_at_ms: Option<i64>,
    login: Option<LoginKey>,
}

pub(super) struct Facts {
    pub(super) active: bool,
    pub(super) deadline: Deadline,
    pub(super) parked: Option<Parked>,
}

impl Roster {
    pub(super) fn of(
        db: &Database,
        home: &Path,
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
            home: home.to_path_buf(),
            profiles,
            categories,
        }
    }

    pub(super) fn recorded(db: &Database, home: &Path, provider: AgentId) -> Self {
        Self::of(
            db,
            home,
            provider,
            &crate::limits::archived(home, provider),
            chrono::Utc::now().timestamp_millis(),
            &|p| Facts {
                active: false,
                deadline: Deadline::recorded(p),
                parked: None,
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

    pub(crate) fn in_column(
        &self,
        profile: &Listed,
        ends: &SignInDeadlines,
        signed_in: Option<&str>,
        now_ms: i64,
    ) -> ProfileDto {
        let active = signed_in == Some(profile.observation_id());
        let listed_end = ends
            .get(profile.observation_id())
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.timestamp_millis());
        let end = if active {
            listed_end
        } else {
            listed_end.or(profile.ends_at_ms)
        };
        stated(
            &profile.profile,
            active,
            &Deadline::at(end),
            self.parked(profile),
            now_ms,
        )
    }

    fn parked(&self, profile: &Listed) -> Option<Parked> {
        super::usage::parked_for(&self.home, &profile.profile.id, profile.login.as_ref()?)
    }

    #[cfg(test)]
    pub(crate) fn park_for_test(&self, account_id: &str, parked: Parked) {
        let profile = self.profile(account_id).expect("a listed profile");
        super::usage::park_for_test(
            &self.home,
            &profile.profile.id,
            profile.login.as_ref().expect("a kept login"),
            parked,
        );
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
}

impl AccountsDto {
    pub(crate) fn sign_in_deadlines(&self) -> SignInDeadlines {
        self.profiles
            .iter()
            .filter_map(|p| Some((p.observation_id.clone(), p.sign_in_ends_at.clone()?)))
            .collect()
    }
}

fn stated(
    profile: &ProfileDto,
    signed_in: bool,
    deadline: &Deadline,
    parked: Option<Parked>,
    now_ms: i64,
) -> ProfileDto {
    let standing = if signed_in {
        Standing::SignedIn
    } else {
        Standing::Saved(parked)
    };
    ProfileDto {
        active: signed_in,
        sign_in_ends_at: deadline.spelled(),
        login_state: LoginState::of(profile.needs_login, deadline, standing, now_ms),
        ..profile.clone()
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
    let profile = ProfileDto {
        sign_in_ends_at: None,
        login_state: LoginState::default(),
        needs_login: p.needs_login(),
        archived: archived.contains(&observation_id),
        category: categories.get(&observation_id).cloned(),
        legacy_id: p.identity.legacy_key(),
        active: false,
        observation_id,
        id: p.id.clone(),
        identity: p.identity.clone(),
        label: p.label.clone(),
        email: p.email.clone(),
        saved_at: p.saved_at.clone(),
        pending_activation: p.pending_activation,
    };
    Listed {
        profile: stated(
            &profile,
            facts.active,
            &facts.deadline,
            facts.parked,
            now_ms,
        ),
        ends_at_ms: facts.deadline.ends_at_ms(),
        login: super::usage::login_key(p),
    }
}

#[cfg(test)]
pub(crate) fn roster_for_test(
    provider: AgentId,
    profiles: &[(super::model::Identity, &str)],
    archived: &[String],
    categories: &[(&str, &str)],
) -> Roster {
    roster_with_logins_for_test(
        Path::new(""),
        provider,
        profiles,
        archived,
        categories,
        &|_| None,
    )
}

#[cfg(test)]
pub(crate) fn roster_with_logins_for_test(
    home: &Path,
    provider: AgentId,
    profiles: &[(super::model::Identity, &str)],
    archived: &[String],
    categories: &[(&str, &str)],
    ends_at_ms: &dyn Fn(&str) -> Option<i64>,
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
        home,
        provider,
        &archived.iter().cloned().collect(),
        0,
        &|p| Facts {
            active: false,
            deadline: Deadline::at(ends_at_ms(p.email.as_deref().unwrap_or_default())),
            parked: None,
        },
    )
}
