use super::{
    model::{Identity, SignInDeadline},
    store::{ChangeKind, Login, Profile, Store},
    usage::Kept,
};
use crate::limits::outcome::Parked;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum LoginState {
    #[default]
    Usable,
    Ending {
        #[serde(rename = "endsAt")]
        ends_at: String,
    },
    Ended {
        #[serde(rename = "endsAt")]
        ends_at: String,
    },
    Refused,
    OtherAccount,
    Missing,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignInEnd {
    #[default]
    Unread,
    At(i64),
    Unknown,
}

impl SignInEnd {
    pub fn is_unread(&self) -> bool {
        matches!(self, Self::Unread)
    }

    pub fn of(identity: &Identity, login: &Login) -> Self {
        let deadline = super::view(identity.provider, login)
            .ok()
            .and_then(|view| view.sign_in_deadline());
        Self::from_deadline(identity, deadline.as_ref())
    }

    fn from_deadline(identity: &Identity, deadline: Option<&SignInDeadline>) -> Self {
        deadline
            .filter(|deadline| deadline.identity == *identity)
            .map_or(Self::Unknown, |deadline| Self::At(deadline.ends_at_ms))
    }

    fn ends_at_ms(self) -> Option<i64> {
        match self {
            Self::At(ends_at_ms) => Some(ends_at_ms),
            Self::Unread | Self::Unknown => None,
        }
    }
}

pub(super) enum Standing {
    SignedIn,
    Saved(Option<Parked>),
}

pub(super) struct Deadline {
    ends_at_ms: Option<i64>,
    read: Option<SignInEnd>,
}

impl Deadline {
    pub(super) fn of(
        profile: &Profile,
        native: Option<&SignInDeadline>,
        kept: Option<&Kept<'_>>,
    ) -> Self {
        let native = native
            .filter(|deadline| deadline.identity == profile.identity)
            .map(|deadline| deadline.ends_at_ms);
        match native {
            Some(ends_at_ms) if !profile.pending_activation => Self {
                ends_at_ms: Some(ends_at_ms),
                read: None,
            },
            _ => Self::saved(profile, kept),
        }
    }

    pub(super) fn recorded(profile: &Profile) -> Self {
        Self::at(profile.sign_in_end.ends_at_ms())
    }

    pub(super) fn at(ends_at_ms: Option<i64>) -> Self {
        Self {
            ends_at_ms,
            read: None,
        }
    }

    pub(super) fn ends_at_ms(&self) -> Option<i64> {
        self.ends_at_ms
    }

    fn saved(profile: &Profile, kept: Option<&Kept<'_>>) -> Self {
        let recorded = |end: SignInEnd| Self {
            ends_at_ms: end.ends_at_ms(),
            read: None,
        };
        match kept {
            Some(Kept::Vault(login)) => recorded(SignInEnd::of(&profile.identity, login)),
            Some(Kept::Home(_, home)) if profile.sign_in_end.is_unread() => {
                let read =
                    SignInEnd::from_deadline(&profile.identity, home.sign_in_deadline().as_ref());
                Self {
                    ends_at_ms: read.ends_at_ms(),
                    read: Some(read),
                }
            }
            Some(Kept::Home(..)) => recorded(profile.sign_in_end),
            None => recorded(SignInEnd::Unknown),
        }
    }

    pub(super) fn read(&self) -> Option<SignInEnd> {
        self.read
    }

    pub(super) fn spelled(&self) -> Option<String> {
        chrono::DateTime::from_timestamp_millis(self.ends_at_ms?)
            .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    }

    fn state(&self, now_ms: i64) -> Option<LoginState> {
        let ends_at = self.spelled()?;
        Some(if self.ends_at_ms? <= now_ms {
            LoginState::Ended { ends_at }
        } else {
            LoginState::Ending { ends_at }
        })
    }
}

impl LoginState {
    pub(super) fn of(
        needs_login: bool,
        deadline: &Deadline,
        standing: Standing,
        now_ms: i64,
    ) -> Self {
        let dated = deadline.state(now_ms);
        match standing {
            Standing::SignedIn => dated.unwrap_or_default(),
            Standing::Saved(_) if needs_login => Self::Missing,
            Standing::Saved(parked) => match (dated, parked) {
                (Some(ended @ Self::Ended { .. }), _) => ended,
                (_, Some(Parked::OtherAccount)) => Self::OtherAccount,
                (_, Some(Parked::Refused)) => Self::Refused,
                (dated, None) => dated.unwrap_or_default(),
            },
        }
    }
}

pub(super) type ReadEnd = (String, Option<String>, SignInEnd);

pub(super) fn remember_ends(home: &std::path::Path, read: &[ReadEnd]) -> Result<(), String> {
    Store::open_existing(home)?.change(ChangeKind::Metadata, |db| {
        for (id, account_home, end) in read {
            if let Some(profile) = db.profiles.iter_mut().find(|profile| {
                profile.id == *id
                    && profile.home == *account_home
                    && profile.sign_in_end.is_unread()
            }) {
                profile.sign_in_end = *end;
            }
        }
        Ok(())
    })
}
