use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

use chrono::{DateTime, Utc};

use super::reset_alerts::{self, Offer};
use crate::dto::{LimitsStatus, PendingResetSpendDto, ProviderLimitsDto, ResetCreditOutcome};
use crate::limits::ResetSpend;
use crate::read_revision::{announce, Source};
use crate::settings::ResetAlert;

pub(super) const DELAY_MINUTES: i64 = 10;

pub(super) const GRACE_MINUTES: i64 = 15;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PendingSpend {
    pub(super) account_id: String,
    pub(super) account_label: Option<String>,
    pub(super) cycle: String,
    pub(super) due_at: DateTime<Utc>,
    pub(super) idempotency_key: String,
}

#[derive(Debug, PartialEq)]
pub(super) enum Due {
    Spend(PendingSpend, ResetSpend),
    Kept(PendingSpend, Kept),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kept {
    NotNeeded,
    Late,
}

enum CodexNow<'a> {
    Live(&'a ProviderLimitsDto),
    NobodySignedIn,
    Unknown,
}

fn codex_now(snapshots: &[ProviderLimitsDto]) -> CodexNow<'_> {
    let current = snapshots.iter().find(|snapshot| {
        snapshot.provider == crate::dto::AgentId::Codex && snapshot.current_account
    });
    match current {
        Some(snapshot) if reset_alerts::is_signed_in_codex(snapshot) => CodexNow::Live(snapshot),
        Some(snapshot) if snapshot.status != LimitsStatus::Failed => CodexNow::NobodySignedIn,
        _ => CodexNow::Unknown,
    }
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

fn due(
    pending: &mut HashMap<String, PendingSpend>,
    snapshots: &[ProviderLimitsDto],
    alerts: &HashMap<String, ResetAlert>,
    now: DateTime<Utc>,
) -> Vec<Due> {
    pending.retain(|account, _| alerts.get(account).is_some_and(|alert| alert.automatic));
    let late = |spend: &PendingSpend| spend.due_at + chrono::Duration::minutes(GRACE_MINUTES) < now;
    let codex = codex_now(snapshots);
    let decided = !matches!(codex, CodexNow::Unknown);
    pending
        .extract_if(|_, spend| spend.due_at <= now && (decided || late(spend)))
        .map(|(account, spend)| {
            if late(&spend) {
                return Due::Kept(spend, Kept::Late);
            }
            let deciding_alert = match codex {
                CodexNow::Live(snapshot) => alerts.get(&account).filter(|alert| {
                    reset_alerts::offer_now(snapshot, alert, now).is_some_and(|offer| {
                        offer.account_id == account && offer.cycle == spend.cycle
                    })
                }),
                CodexNow::NobodySignedIn | CodexNow::Unknown => None,
            };
            match deciding_alert {
                Some(alert) => Due::Spend(
                    spend,
                    ResetSpend::Automatic {
                        max_left_percent: alert.spend_limit(),
                    },
                ),
                None => Due::Kept(spend, Kept::NotNeeded),
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

pub(super) fn kept_copy(spend: &PendingSpend, why: Kept) -> (String, String) {
    let why = match why {
        Kept::NotNeeded => "no longer needs it, or is no longer signed in",
        Kept::Late => "could not be checked in time",
    };
    (
        "Codex: banked reset not used".to_string(),
        format!(
            "{} {why}, so it was kept.",
            name(spend.account_label.as_deref())
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

static PENDING: LazyLock<Mutex<HashMap<String, PendingSpend>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn with_pending<R>(change: impl FnOnce(&mut HashMap<String, PendingSpend>) -> R) -> R {
    change(&mut PENDING.lock().unwrap_or_else(PoisonError::into_inner))
}

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

pub(super) fn any_due(now: DateTime<Utc>) -> bool {
    with_pending(|pending| pending.values().any(|spend| spend.due_at <= now))
}

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

pub(super) fn next_due() -> Option<DateTime<Utc>> {
    with_pending(|pending| pending.values().map(|spend| spend.due_at).min())
}

pub(super) fn clear() -> bool {
    let cleared = with_pending(|pending| !std::mem::take(pending).is_empty());
    if cleared {
        announce(Source::ResetSpends);
    }
    cleared
}

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

pub fn used_by_hand(account_id: &str, outcome: ResetCreditOutcome) {
    if outcome == ResetCreditOutcome::Reset {
        cancel_listed(account_id);
    }
}

pub fn cancel_listed(account_id: &str) -> bool {
    let cancelled = with_pending(|pending| pending.remove(account_id).is_some());
    if cancelled {
        announce(Source::ResetSpends);
    }
    cancelled
}

#[cfg(test)]
mod tests;
