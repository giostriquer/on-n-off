//! A banked reset used by itself, for an account whose alert says so (`ResetAlert::automatic`).
//!
//! When such an alert makes its offer (`reset_alerts::observe`: two low polls in a row, far enough
//! from renewal, once per weekly cycle), the reset is not spent there and then. The user is told it
//! will be used in [`DELAY_MINUTES`], and can cancel it on the account's card until then. At that
//! time it is spent only if the latest read still finds the signed-in account low in the same
//! weekly cycle, and the spend itself keeps every rule a click on the card keeps
//! (`limits_refresh::consume_codex_reset_credit`). A spend that fails is never tried again by
//! itself: the cycle's offer has been made.
//!
//! What is waiting lives in memory only, so quitting on-n-off cancels it: a reset is never spent
//! by an app started long after the user was told.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

use chrono::{DateTime, Utc};

use super::reset_alerts::{self, Offer};
use crate::dto::{PendingResetSpendDto, ProviderLimitsDto, ResetCreditOutcome};
use crate::settings::ResetAlert;

/// How long after the user is told a reset is spent, for them to cancel it.
pub(super) const DELAY_MINUTES: i64 = 10;

/// A reset waiting to be spent.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PendingSpend {
    pub(super) account_id: String,
    pub(super) account_label: Option<String>,
    /// The weekly cycle it was offered for; it is spent for that cycle or not at all.
    pub(super) cycle: String,
    pub(super) due_at: DateTime<Utc>,
    /// One key per spend, so a spend Codex was slow to answer is never made twice.
    pub(super) idempotency_key: String,
}

/// What comes of a spend whose time has come.
#[derive(Debug, PartialEq)]
pub(super) enum Due {
    Spend(PendingSpend),
    NotNeeded(PendingSpend),
}

/// The spend `offer` becomes at `now`.
pub(super) fn schedule(offer: &Offer, now: DateTime<Utc>) -> PendingSpend {
    PendingSpend {
        account_id: offer.account_id.clone(),
        account_label: offer.account_label.clone(),
        cycle: offer.cycle.clone(),
        due_at: now + chrono::Duration::minutes(DELAY_MINUTES),
        idempotency_key: uuid::Uuid::new_v4().to_string(),
    }
}

/// Every spend in `pending` whose time has come by `now`, taken out of it: to be spent when
/// `snapshots` still find its account low in the same cycle, otherwise not needed any more. A
/// spend whose alert was turned off, or no longer spends by itself, is dropped without a word.
pub(super) fn due(
    pending: &mut HashMap<String, PendingSpend>,
    snapshots: &[ProviderLimitsDto],
    alerts: &HashMap<String, ResetAlert>,
    now: DateTime<Utc>,
) -> Vec<Due> {
    pending.retain(|account, _| alerts.get(account).is_some_and(|alert| alert.automatic));
    let ready: Vec<String> = pending
        .iter()
        .filter(|(_, spend)| spend.due_at <= now)
        .map(|(account, _)| account.clone())
        .collect();
    ready
        .into_iter()
        .filter_map(|account| {
            let spend = pending.remove(&account)?;
            let alert = alerts.get(&account)?;
            let still_needed = snapshots.iter().any(|snapshot| {
                reset_alerts::offer_now(snapshot, alert, now)
                    .is_some_and(|offer| offer.account_id == account && offer.cycle == spend.cycle)
            });
            Some(if still_needed {
                Due::Spend(spend)
            } else {
                Due::NotNeeded(spend)
            })
        })
        .collect()
}

/// Takes `account_id`'s waiting spend out of `pending`; `false` when there was none.
pub(super) fn cancel(pending: &mut HashMap<String, PendingSpend>, account_id: &str) -> bool {
    pending.remove(account_id).is_some()
}

/// When the soonest waiting spend is due, for the monitor to wake then.
pub(super) fn next_due(pending: &HashMap<String, PendingSpend>) -> Option<DateTime<Utc>> {
    pending.values().map(|spend| spend.due_at).min()
}

fn name(label: Option<&str>) -> &str {
    label.unwrap_or("The Codex account")
}

pub(super) fn scheduled_copy(offer: &Offer) -> (String, String) {
    (
        format!("Codex: using a banked reset in {DELAY_MINUTES} minutes"),
        format!(
            "{} has {:.0}% of its limit left. Cancel on the account's card in on-n-off to keep the reset.",
            name(offer.account_label.as_deref()),
            offer.left_percent
        ),
    )
}

pub(super) fn outcome_copy(
    spend: &PendingSpend,
    outcome: &Result<ResetCreditOutcome, String>,
) -> (String, String) {
    let account = name(spend.account_label.as_deref());
    let not_used = |why: String| ("Codex: banked reset not used".to_string(), why);
    match outcome {
        Ok(ResetCreditOutcome::Reset) => (
            "Codex: banked reset used".to_string(),
            format!("{account}'s usage is back to 0%."),
        ),
        Ok(ResetCreditOutcome::NothingToReset) => {
            not_used(format!("{account}'s usage was already at 0%."))
        }
        Ok(ResetCreditOutcome::NoCredit) => not_used(format!("{account} has no banked reset left.")),
        Ok(ResetCreditOutcome::AlreadyRedeemed) => {
            not_used("That banked reset was already used.".to_string())
        }
        Ok(ResetCreditOutcome::Unknown) => not_used(
            "Codex answered with a result on-n-off doesn't recognize. Check the reset count on the card."
                .to_string(),
        ),
        Err(why) => not_used(why.clone()),
    }
}

pub(super) fn not_needed_copy(spend: &PendingSpend) -> (String, String) {
    (
        "Codex: banked reset not used".to_string(),
        format!(
            "{} no longer needs it, or is no longer signed in, so it was kept.",
            name(spend.account_label.as_deref())
        ),
    )
}

/// The spends waiting in this app, which the monitor schedules and spends and the account's card
/// lists and cancels.
static PENDING: LazyLock<Mutex<HashMap<String, PendingSpend>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Runs `change` on the waiting spends, holding their lock only for it: `change` does no I/O.
pub(super) fn with_pending<R>(change: impl FnOnce(&mut HashMap<String, PendingSpend>) -> R) -> R {
    change(&mut PENDING.lock().unwrap_or_else(PoisonError::into_inner))
}

/// The waiting spends as the card shows them.
pub fn listed() -> Vec<PendingResetSpendDto> {
    with_pending(|pending| {
        pending
            .values()
            .map(|spend| PendingResetSpendDto {
                account_id: spend.account_id.clone(),
                due_at: spend
                    .due_at
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            })
            .collect()
    })
}

/// Cancels `account_id`'s waiting spend, from its card; `false` when there was none, or it had
/// already begun.
pub fn cancel_listed(account_id: &str) -> bool {
    let cancelled = with_pending(|pending| cancel(pending, account_id));
    if cancelled {
        crate::read_revision::announce(crate::read_revision::Source::ResetSpends);
    }
    cancelled
}

#[cfg(test)]
mod tests;
