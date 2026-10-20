use super::{homes::Resolve, model::SignInDeadline, store::Profile};
use crate::limits::outcome::Parked;
use serde::Serialize;

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

pub(super) struct Deadline(Option<i64>);

impl Deadline {
    pub(super) fn of(
        profile: &Profile,
        native: Option<&SignInDeadline>,
        homes: Option<&Resolve<'_>>,
    ) -> Self {
        let own = |deadline: &SignInDeadline| {
            (deadline.identity == profile.identity).then_some(deadline.ends_at_ms)
        };
        let native = || native.and_then(own);
        let saved = || {
            let vault = profile.login.as_ref().and_then(|login| {
                super::view(profile.identity.provider, login)
                    .ok()?
                    .sign_in_deadline()
            });
            vault
                .or_else(|| homes?(profile.home.as_deref()?).ok()?.sign_in_deadline())
                .as_ref()
                .and_then(own)
        };
        Self(if profile.pending_activation {
            saved()
        } else {
            native().or_else(saved)
        })
    }

    pub(super) fn spelled(&self) -> Option<String> {
        chrono::DateTime::from_timestamp_millis(self.0?)
            .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    }
}

impl LoginState {
    pub(super) fn of(
        profile: &Profile,
        deadline: &Deadline,
        parked: Option<Parked>,
        now_ms: i64,
    ) -> Self {
        if profile.needs_login() {
            return Self::Missing;
        }
        let ends_at = deadline.spelled();
        let ended = deadline.0.is_some_and(|ends_at_ms| ends_at_ms <= now_ms);
        match (ends_at, parked) {
            (Some(ends_at), _) if ended => Self::Ended { ends_at },
            (_, Some(Parked::OtherAccount)) => Self::OtherAccount,
            (_, Some(Parked::Refused)) => Self::Refused,
            (Some(ends_at), None) => Self::Ending { ends_at },
            (None, None) => Self::Usable,
        }
    }
}
