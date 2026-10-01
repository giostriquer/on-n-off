use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::dto::{AgentId, LimitWindowKind, LimitsAccountDto, LimitsStatus, ProviderLimitsDto};
use crate::settings::ResetAlert;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AlertState {
    #[serde(default)]
    low: Option<LowReading>,
    #[serde(default)]
    offered_cycle: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LowReading {
    cycle: String,
    observed_at: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Offer {
    pub(super) account_id: String,
    pub(super) account_label: Option<String>,
    pub(super) cycle: String,
    pub(super) left_percent: f64,
    pub(super) renews_at: DateTime<Utc>,
    pub(super) available: u32,
    pub(super) automatic: bool,
}

pub(super) fn observe(
    state: &mut HashMap<String, AlertState>,
    snapshots: &[ProviderLimitsDto],
    alerts: &HashMap<String, ResetAlert>,
    now: DateTime<Utc>,
) -> Vec<Offer> {
    state.retain(|account, _| alerts.contains_key(account));
    let mut offers = Vec::new();
    for snapshot in snapshots {
        let Some(account) = signed_in_codex(snapshot) else {
            continue;
        };
        let Some(alert) = alerts.get(&account.id) else {
            continue;
        };
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

pub(super) fn offer_now(
    snapshot: &ProviderLimitsDto,
    alert: &ResetAlert,
    now: DateTime<Utc>,
) -> Option<Offer> {
    signed_in_codex(snapshot)?;
    low_reading(snapshot, alert, now).map(|(_, offer)| offer)
}

pub(super) fn is_signed_in_codex(snapshot: &ProviderLimitsDto) -> bool {
    signed_in_codex(snapshot).is_some()
}

fn signed_in_codex(snapshot: &ProviderLimitsDto) -> Option<&LimitsAccountDto> {
    let account = snapshot.account.as_ref()?;
    (snapshot.provider == AgentId::Codex
        && snapshot.current_account
        && snapshot.status == LimitsStatus::Ok)
        .then_some(account)
}

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
            account_id: snapshot.account.as_ref()?.id.clone(),
            account_label: snapshot
                .account
                .as_ref()
                .and_then(|account| account.label.clone()),
            cycle: cycle.to_string(),
            left_percent: left,
            renews_at,
            available: banked.available_count,
            automatic: alert.automatic,
        },
    ))
}

fn instant(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

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
