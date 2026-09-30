//! When to offer a banked Codex reset: an account the user opted in (`settings::ResetAlert`) that
//! has run low, long enough before its limit renews by itself that a reset is worth spending. The
//! offer is a notification; the reset is still the user's to spend, on the account's card, as in
//! Codex's own app, which never uses one automatically.
//!
//! Low is judged as the spend itself judges it: what is left of the current limit
//! (`Reading::limit_left_percent`), at the account's share (`ResetAlert::spend_limit`). The offer
//! needs two polls in a row to find the
//! account low in the same weekly cycle, each a new read, so one stray reading never offers a
//! reset; and it is made once per weekly cycle, which a spent reset starts anew.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::dto::{AgentId, LimitWindowKind, LimitsStatus, ProviderLimitsDto};
use crate::settings::ResetAlert;

/// What the monitor remembers of one account's alert between polls, by the card's account id.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AlertState {
    /// The weekly cycle, by its reset instant, a poll last found the account low in, and when that
    /// reading was made: the next poll must find it low again, in a newer reading.
    #[serde(default)]
    low: Option<LowReading>,
    /// The weekly cycle a reset was last offered for.
    #[serde(default)]
    offered_cycle: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LowReading {
    cycle: String,
    observed_at: String,
}

/// A banked reset worth offering.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Offer {
    pub(super) account_label: Option<String>,
    /// What is left of the current limit, 0 to 100.
    pub(super) left_percent: f64,
    /// When the weekly window renews by itself.
    pub(super) renews_at: DateTime<Utc>,
    pub(super) available: u32,
}

/// Every offer `snapshots` make at `now` for the accounts in `alerts`, with `state` brought up to
/// date. An account no longer opted in is forgotten.
pub(super) fn observe(
    state: &mut HashMap<String, AlertState>,
    snapshots: &[ProviderLimitsDto],
    alerts: &HashMap<String, ResetAlert>,
    now: DateTime<Utc>,
) -> Vec<Offer> {
    state.retain(|account, _| alerts.contains_key(account));
    let mut offers = Vec::new();
    for snapshot in snapshots {
        let Some(account) = snapshot.account.as_ref() else {
            continue;
        };
        let Some(alert) = alerts.get(&account.id) else {
            continue;
        };
        if snapshot.provider != AgentId::Codex
            || !snapshot.current_account
            || snapshot.status != LimitsStatus::Ok
        {
            continue;
        }
        let entry = state.entry(account.id.clone()).or_default();
        match low_reading(snapshot, alert, now) {
            None => entry.low = None,
            Some((reading, offer)) => {
                let confirmed = entry.low.as_ref().is_some_and(|low| {
                    low.cycle == reading.cycle && is_newer(&reading.observed_at, &low.observed_at)
                });
                let offered = entry.offered_cycle.as_deref() == Some(reading.cycle.as_str());
                if confirmed && !offered {
                    entry.offered_cycle = Some(reading.cycle.clone());
                    offers.push(offer);
                }
                entry.low = Some(reading);
            }
        }
    }
    offers
}

/// The low reading `snapshot` makes for `alert` at `now`, and the offer it would be, when all hold:
/// no more than the alert's share of the current limit is left, the weekly window renews by itself
/// no sooner than the alert's wait, and a banked reset is available.
fn low_reading(
    snapshot: &ProviderLimitsDto,
    alert: &ResetAlert,
    now: DateTime<Utc>,
) -> Option<(LowReading, Offer)> {
    let left = snapshot.reading.limit_left_percent()?;
    if left > f64::from(alert.spend_limit()) {
        return None;
    }
    let weekly = snapshot
        .reading
        .windows
        .iter()
        .find(|window| window.kind == LimitWindowKind::Weekly)?;
    let cycle = weekly.resets_at.as_deref()?;
    let renews_at = instant(cycle)?;
    if renews_at - now < chrono::Duration::hours(i64::from(alert.min_hours_to_renewal)) {
        return None;
    }
    let banked = snapshot.reading.reset_credits.as_ref()?;
    let lapsed = banked
        .next_expires_at
        .as_deref()
        .and_then(instant)
        .is_some_and(|at| at <= now);
    if banked.available_count == 0 || lapsed {
        return None;
    }
    Some((
        LowReading {
            cycle: cycle.to_string(),
            observed_at: weekly.observed_at.clone(),
        },
        Offer {
            account_label: snapshot
                .account
                .as_ref()
                .and_then(|account| account.label.clone()),
            left_percent: left,
            renews_at,
            available: banked.available_count,
        },
    ))
}

fn instant(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// Whether `later` was observed after `earlier`; an observation time that cannot be read is never
/// newer.
fn is_newer(later: &str, earlier: &str) -> bool {
    instant(later)
        .zip(instant(earlier))
        .is_some_and(|(later, earlier)| later > earlier)
}

pub(super) fn notification_copy(offer: &Offer, now: DateTime<Utc>) -> (String, String) {
    let account = offer
        .account_label
        .as_deref()
        .map(|label| format!("{label} has "))
        .unwrap_or_else(|| "Codex has ".to_string());
    let hours = (offer.renews_at - now).num_hours().max(0);
    let renews = if hours >= 24 {
        format!("{}d {}h", hours / 24, hours % 24)
    } else {
        format!("{hours}h")
    };
    (
        "Codex: a banked reset is available".to_string(),
        format!(
            "{account}{:.0}% of its limit left and renews in {renews}. Open on-n-off to use a banked reset.",
            offer.left_percent
        ),
    )
}

#[cfg(test)]
mod tests;
