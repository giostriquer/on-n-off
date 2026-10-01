use chrono::{DateTime, SecondsFormat, Utc};

use crate::dto::{LimitWindowDto, LimitWindowKind, LimitsStatus, ProviderLimitsDto, Reading};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Answered {
        asked_what_was_spent: bool,
        asked_about_renewal: bool,
    },
    Failed,
}

impl Outcome {
    fn answered(card: &ProviderLimitsDto) -> Self {
        Self::Answered {
            asked_what_was_spent: super::credits_spent::asks_what_was_spent(card),
            asked_about_renewal: super::renewal::asks_about_renewal(card),
        }
    }

    fn of(card: &ProviderLimitsDto) -> Self {
        if card.status == LimitsStatus::Ok {
            Self::answered(card)
        } else {
            Self::Failed
        }
    }
}

pub(crate) fn keep_remembered(card: &mut ProviderLimitsDto, remembered: Reading) {
    let outcome = Outcome::of(card);
    card.reading = std::mem::take(&mut card.reading).keeping(remembered, outcome);
}

impl Reading {
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
            windows: if failed {
                merged(windows, remembered_windows)
            } else {
                with_remembered_weekly(windows, remembered_windows)
            },
            credits: kept(credits, remembered_credits, failed),
            workspace_credits: kept(workspace_credits, remembered_share, failed),
            credits_spent: kept(credits_spent, remembered_spent, keeps_spent),
            subscription: kept(subscription, remembered_term, keeps_term),
            reset_credits: kept(reset_credits, remembered_resets, true),
            reset_offer,
        }
    }

    pub(super) fn known_at(self, now: DateTime<Utc>) -> Self {
        Self {
            reset_credits: self
                .reset_credits
                .filter(|resets| !passed(resets.next_expires_at.as_deref(), now)),
            reset_offer: None,
            ..self
        }
    }

    fn can_stand_in(&self) -> bool {
        self.has_observations() && (self.windows.is_empty() || newest(&self.windows).is_some())
    }
}

fn passed(at: Option<&str>, now: DateTime<Utc>) -> bool {
    at.and_then(parse_observed_at).is_some_and(|at| at <= now)
}

fn kept<T>(own: Option<T>, remembered: Option<T>, keep: bool) -> Option<T> {
    own.or(if keep { remembered } else { None })
}

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
