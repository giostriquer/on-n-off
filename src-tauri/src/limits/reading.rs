//! The remember policy: what a read of an account keeps of the account's remembered reading.
//!
//! A read applies it once, keeping from its account's file as it writes over it
//! (`SnapshotStore::remember`): the signed-in read (`aggregate_accounts`), a saved profile's poll
//! and the first usage after a sign-in. A saved profile's poll that failed writes nothing, so it
//! keeps from the card it replaces (`accounts/usage.rs`). Each field's rule is chosen once, in
//! [`Reading::keeping`]:
//!
//! - **Windows**: an answer's own. An answer that reports other windows but no weekly also keeps
//!   the remembered weekly, dated when it was read: a card's headline window is its weekly, and a
//!   card that misses it keeps the last one read rather than lead with its session. An answer
//!   that reports no windows at all keeps none. A failed read keeps its own with the remembered
//!   ones merged in by id, the newer observation of each winning.
//! - **Plan, subscription status, credits, workspace credits**: an answer's own, absent included.
//!   A failed read keeps its own, else the remembered ones.
//! - **Credits spent, subscription term**: fetched beside the usage read, which may not have been
//!   able to tell them, so an answer keeps the remembered one where it has none and the card is
//!   asked for it (`asks_what_was_spent`, `asks_about_renewal`). A failed read keeps its own,
//!   else the remembered ones.
//! - **Banked resets**: its own, else the remembered count; a count a read could not tell is
//!   unknown, never 0. A remembered count is known only until its soonest expiry: the store
//!   loads, and keeps from, what a remembered reading still knows now ([`Reading::known_at`]).
//! - **Reset offer**: its own, never the remembered one.

use chrono::{DateTime, SecondsFormat, Utc};

use crate::dto::{LimitWindowDto, LimitWindowKind, LimitsStatus, ProviderLimitsDto, Reading};

/// How the read behind a reading went, which decides the column of the policy that applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The read answered, so what it left out is its answer, except for the figures fetched beside
    /// the usage read that the card is asked for: those it may simply not have been able to tell.
    Answered {
        asked_what_was_spent: bool,
        asked_about_renewal: bool,
    },
    /// The read failed and said nothing, so the remembered reading stands in for it.
    Failed,
}

impl Outcome {
    /// A read of `card` that answered.
    fn answered(card: &ProviderLimitsDto) -> Self {
        Self::Answered {
            asked_what_was_spent: super::credits_spent::asks_what_was_spent(card),
            asked_about_renewal: super::renewal::asks_about_renewal(card),
        }
    }

    /// How `card`'s read went, as its status records it.
    fn of(card: &ProviderLimitsDto) -> Self {
        if card.status == LimitsStatus::Ok {
            Self::answered(card)
        } else {
            Self::Failed
        }
    }
}

/// `card`, keeping what its read could not tell from the account's `remembered` reading.
pub(crate) fn keep_remembered(card: &mut ProviderLimitsDto, remembered: Reading) {
    let outcome = Outcome::of(card);
    card.reading = std::mem::take(&mut card.reading).keeping(remembered, outcome);
}

impl Reading {
    /// This reading, with `remembered`'s values wherever a read that went as `outcome` could not
    /// tell. Every field of both is named, so a field added to `Reading` does not compile until
    /// its rule is chosen here.
    pub(crate) fn keeping(self, remembered: Self, outcome: Outcome) -> Self {
        let failed = outcome == Outcome::Failed;
        let remembered = if failed && !remembered.can_stand_in() {
            Self::default()
        } else {
            remembered
        };
        let (keeps_spent, keeps_term) = match outcome {
            Outcome::Answered {
                asked_what_was_spent,
                asked_about_renewal,
            } => (asked_what_was_spent, asked_about_renewal),
            Outcome::Failed => (true, true),
        };
        let Self {
            plan,
            subscription_status,
            windows,
            credits,
            workspace_credits,
            credits_spent,
            subscription,
            reset_credits,
            reset_offer,
        } = self;
        let Self {
            plan: remembered_plan,
            subscription_status: remembered_status,
            windows: remembered_windows,
            credits: remembered_credits,
            workspace_credits: remembered_share,
            credits_spent: remembered_spent,
            subscription: remembered_term,
            reset_credits: remembered_resets,
            reset_offer: _,
        } = remembered;
        Self {
            plan: kept(plan, remembered_plan, failed),
            subscription_status: kept(subscription_status, remembered_status, failed),
            windows: if failed {
                merged(windows, remembered_windows)
            } else {
                with_remembered_weekly(windows, remembered_windows)
            },
            credits: kept(credits, remembered_credits, failed),
            workspace_credits: kept(workspace_credits, remembered_share, failed),
            credits_spent: kept(credits_spent, remembered_spent, keeps_spent),
            subscription: kept(subscription, remembered_term, keeps_term),
            // A count a read could not tell is unknown, never 0.
            reset_credits: kept(reset_credits, remembered_resets, true),
            // An offer is in flight: withdrawn between reads, it must go with them.
            reset_offer,
        }
    }

    /// What this remembered reading still says at `now`, for the card that shows it and for the
    /// save that keeps from it alike. A banked-reset count whose soonest known expiry has passed is
    /// no longer known, and a live offer belongs to the read that saw it. A share past its reset
    /// has renewed, which the card shows as it shows a window's passed reset; it is kept, since
    /// dropping it would bring back the own balance of 0.
    pub(super) fn known_at(self, now: DateTime<Utc>) -> Self {
        Self {
            reset_credits: self
                .reset_credits
                .filter(|resets| !passed(resets.next_expires_at.as_deref(), now)),
            reset_offer: None,
            ..self
        }
    }

    /// Whether this remembered reading can stand in for a failed read: it observed something, and
    /// its windows, if it has any, carry a time to be merged by.
    fn can_stand_in(&self) -> bool {
        self.has_observations() && (self.windows.is_empty() || newest(&self.windows).is_some())
    }
}

/// Whether a remembered banked-reset count's soonest known expiry has come: by then at least one
/// reset has lapsed and what is left is not known until a read answers again. The UI applies the
/// same rule to a card already on screen (`unexpiredBankedResets`).
fn passed(at: Option<&str>, now: DateTime<Utc>) -> bool {
    at.and_then(parse_observed_at).is_some_and(|at| at <= now)
}

fn kept<T>(own: Option<T>, remembered: Option<T>, keep: bool) -> Option<T> {
    own.or(if keep { remembered } else { None })
}

/// A failed read's own windows with the remembered ones merged in by id: of two observations of
/// one window the newer wins, and one whose time cannot be read never replaces the other. A
/// remembered window without a time takes the newest one its reading has. Weekly first, then
/// session, then model.
fn merged(
    mut windows: Vec<LimitWindowDto>,
    remembered: Vec<LimitWindowDto>,
) -> Vec<LimitWindowDto> {
    let newest = newest(&remembered).map(|at| at.to_rfc3339_opts(SecondsFormat::Millis, true));
    for mut incoming in remembered {
        if let (true, Some(newest)) = (incoming.observed_at.is_empty(), &newest) {
            incoming.observed_at.clone_from(newest);
        }
        match windows.iter().position(|window| window.id == incoming.id) {
            Some(index) if is_newer(&incoming, &windows[index]) => windows[index] = incoming,
            Some(_) => {}
            None => windows.push(incoming),
        }
    }
    windows.sort_by_key(|window| super::pipeline::kind_rank(window.kind));
    windows
}

/// An answer's own windows, with the remembered weekly window, as it was observed, when the answer
/// reports other windows but no weekly. An answer with no windows at all keeps none: a read of
/// figures alone clears the windows it no longer reports. Weekly first, then session, then model.
/// A carried weekly never lapses on its own: it stays until a read reports a weekly again or the
/// user removes the account, and past its reset it reads as a window that has reset.
fn with_remembered_weekly(
    mut windows: Vec<LimitWindowDto>,
    remembered: Vec<LimitWindowDto>,
) -> Vec<LimitWindowDto> {
    let is_weekly = |window: &LimitWindowDto| window.kind == LimitWindowKind::Weekly;
    if !windows.is_empty() && !windows.iter().any(is_weekly) {
        windows.extend(remembered.into_iter().filter(is_weekly));
        windows.sort_by_key(|window| super::pipeline::kind_rank(window.kind));
    }
    windows
}

fn is_newer(incoming: &LimitWindowDto, existing: &LimitWindowDto) -> bool {
    match (
        parse_observed_at(&incoming.observed_at),
        parse_observed_at(&existing.observed_at),
    ) {
        (Some(incoming), Some(existing)) => incoming > existing,
        _ => false,
    }
}

/// The newest time any of `windows` was observed, among those whose time can be read.
pub(super) fn newest(windows: &[LimitWindowDto]) -> Option<DateTime<Utc>> {
    windows
        .iter()
        .filter_map(|window| parse_observed_at(&window.observed_at))
        .max()
}

pub(super) fn parse_observed_at(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|observed_at| observed_at.with_timezone(&Utc))
}

#[cfg(test)]
mod tests;
