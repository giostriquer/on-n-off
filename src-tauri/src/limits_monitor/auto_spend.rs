//! A banked reset used by itself, for an account whose alert says so (`ResetAlert::automatic`).
//!
//! When such an alert makes its offer (`reset_alerts::observe`: two low polls in a row, far enough
//! from renewal, once per weekly cycle), the reset is not spent there and then. The user is told it
//! will be used in [`DELAY_MINUTES`], and can cancel it on the account's card until then. When the
//! time comes the monitor reads Codex afresh, and the reset is spent only if that read still finds
//! the signed-in account low in the same weekly cycle; the spend itself then keeps every rule a
//! click on the card keeps (`limits_refresh::consume_codex_reset_credit`), and one that fails is
//! never tried again by itself, since the cycle's offer has been made. A read that fails leaves the
//! spend waiting for one that answers.
//!
//! What is waiting lives in memory only, apart from the monitor's saved state, so quitting on-n-off
//! cancels it; and a spend found more than [`GRACE_MINUTES`] past its time, as after the computer
//! slept, is kept rather than spent: a reset is never used long after the user was told.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

use chrono::{DateTime, Utc};

use super::reset_alerts::{self, Offer};
use crate::dto::{PendingResetSpendDto, ProviderLimitsDto, ResetCreditOutcome};
use crate::read_revision::{announce, Source};
use crate::settings::ResetAlert;

/// How long after the user is told a reset is spent, for them to cancel it.
pub(super) const DELAY_MINUTES: i64 = 10;

/// How late a spend may still be made; one found later than this is kept.
pub(super) const GRACE_MINUTES: i64 = 15;

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
    /// The signed-in account's live read no longer finds it low in the same cycle.
    NotNeeded(PendingSpend),
    /// Found more than [`GRACE_MINUTES`] past its time.
    Late(PendingSpend),
}

fn schedule(offer: &Offer, now: DateTime<Utc>) -> PendingSpend {
    PendingSpend {
        account_id: offer.account_id.clone(),
        account_label: offer.account_label.clone(),
        cycle: offer.cycle.clone(),
        due_at: now + chrono::Duration::minutes(DELAY_MINUTES),
        idempotency_key: uuid::Uuid::new_v4().to_string(),
    }
}

/// Every spend in `pending` whose time has come by `now` and can be decided, taken out of it. A
/// spend is decided by the signed-in Codex account's live read in `snapshots`: spent when that read
/// still finds its account low in the same cycle, otherwise not needed. Without a live read it
/// waits, until it is late. A spend whose alert was turned off, or no longer spends by itself, is
/// dropped without a word.
fn due(
    pending: &mut HashMap<String, PendingSpend>,
    snapshots: &[ProviderLimitsDto],
    alerts: &HashMap<String, ResetAlert>,
    now: DateTime<Utc>,
) -> Vec<Due> {
    pending.retain(|account, _| alerts.get(account).is_some_and(|alert| alert.automatic));
    let late = |spend: &PendingSpend| spend.due_at + chrono::Duration::minutes(GRACE_MINUTES) < now;
    let live = snapshots
        .iter()
        .find(|snapshot| reset_alerts::is_signed_in_codex(snapshot));
    pending
        .extract_if(|_, spend| spend.due_at <= now && (live.is_some() || late(spend)))
        .map(|(account, spend)| {
            if late(&spend) {
                return Due::Late(spend);
            }
            let still_needed = live
                .zip(alerts.get(&account))
                .and_then(|(snapshot, alert)| reset_alerts::offer_now(snapshot, alert, now))
                .is_some_and(|offer| offer.account_id == account && offer.cycle == spend.cycle);
            if still_needed {
                Due::Spend(spend)
            } else {
                Due::NotNeeded(spend)
            }
        })
        .collect()
}

fn name(label: Option<&str>) -> &str {
    label.unwrap_or("The Codex account")
}

fn scheduled_copy(offer: &Offer) -> (String, String) {
    (
        format!("Codex: using a banked reset in {DELAY_MINUTES} minutes"),
        format!(
            "{} has {:.0}% of its limit left. Cancel on the account's card in on-n-off to keep the reset.",
            name(offer.account_label.as_deref()),
            offer.left_percent
        ),
    )
}

/// What the notification for a spend that was not made says.
pub(super) fn kept_copy(due: &Due) -> Option<(String, String)> {
    let (spend, why) = match due {
        Due::Spend(_) => return None,
        Due::NotNeeded(spend) => (spend, "no longer needs it, or is no longer signed in"),
        Due::Late(spend) => (spend, "could not be checked in time"),
    };
    Some((
        "Codex: banked reset not used".to_string(),
        format!(
            "{} {why}, so it was kept.",
            name(spend.account_label.as_deref())
        ),
    ))
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

/// The spends waiting in this app, which the monitor schedules and spends and the account's card
/// lists and cancels. Every function here holds the lock only for work on the map itself.
static PENDING: LazyLock<Mutex<HashMap<String, PendingSpend>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn with_pending<R>(change: impl FnOnce(&mut HashMap<String, PendingSpend>) -> R) -> R {
    change(&mut PENDING.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Waits `offers` to be spent in [`DELAY_MINUTES`], and what to tell the user of each. The monitor
/// calls it once the offers are saved, so an offer that failed to save is never scheduled.
pub(super) fn schedule_all(offers: &[Offer], now: DateTime<Utc>) -> Vec<(String, String)> {
    if offers.is_empty() {
        return Vec::new();
    }
    with_pending(|pending| {
        for offer in offers {
            let spend = schedule(offer, now);
            pending.insert(spend.account_id.clone(), spend);
        }
    });
    announce(Source::ResetSpends);
    offers.iter().map(scheduled_copy).collect()
}

/// Whether a spend's time has come by `now`, for the monitor to read Codex afresh for it.
pub(super) fn any_due(now: DateTime<Utc>) -> bool {
    with_pending(|pending| pending.values().any(|spend| spend.due_at <= now))
}

/// Takes every spend that can be decided at `now` out of the waiting ones ([`due`]).
pub(super) fn take_due(
    snapshots: &[ProviderLimitsDto],
    alerts: &HashMap<String, ResetAlert>,
    now: DateTime<Utc>,
) -> Vec<Due> {
    let (taken, changed) = with_pending(|pending| {
        let before = pending.len();
        let taken = due(pending, snapshots, alerts, now);
        (taken, pending.len() != before)
    });
    if changed {
        announce(Source::ResetSpends);
    }
    taken
}

/// When the soonest waiting spend is due, for the monitor to wake then.
pub(super) fn next_due() -> Option<DateTime<Utc>> {
    with_pending(|pending| pending.values().map(|spend| spend.due_at).min())
}

/// Drops every waiting spend, as when no alert is left; `true` when there was one.
pub(super) fn clear() -> bool {
    let cleared = with_pending(|pending| !std::mem::take(pending).is_empty());
    if cleared {
        announce(Source::ResetSpends);
    }
    cleared
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

/// Cancels `account_id`'s waiting spend, from its card or once it was used by hand; `false` when
/// there was none, or it had already begun.
pub fn cancel_listed(account_id: &str) -> bool {
    let cancelled = with_pending(|pending| pending.remove(account_id).is_some());
    if cancelled {
        announce(Source::ResetSpends);
    }
    cancelled
}

#[cfg(test)]
mod tests;
