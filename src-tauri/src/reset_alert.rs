use std::collections::HashMap;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::dto::{
    AgentId, LimitWindowKind, LimitsAccountDto, LimitsStatus, PendingResetSpendDto,
    ProviderLimitsDto, Reading, ResetCreditOutcome,
};
use crate::limits::same_reset_instant;

const SPEND_CAP_PERCENT: u8 = 5;
const LOWEST_SHARE_PERCENT: u8 = 1;
const DEFAULT_HOURS_TO_RENEWAL: u16 = 24;
const MOST_HOURS_TO_RENEWAL: u16 = 7 * 24;
const SPEND_DELAY_MINUTES: i64 = 10;
const LATE_AFTER_MINUTES: i64 = 15;

pub type Notice = (String, String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetAlert {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_share")]
    pub max_left_percent: u8,
    #[serde(default = "default_hours_to_renewal")]
    pub min_hours_to_renewal: u16,
    #[serde(default)]
    pub automatic: bool,
}

const fn default_share() -> u8 {
    SPEND_CAP_PERCENT
}

const fn default_hours_to_renewal() -> u16 {
    DEFAULT_HOURS_TO_RENEWAL
}

impl Default for ResetAlert {
    fn default() -> Self {
        Self {
            label: None,
            max_left_percent: default_share(),
            min_hours_to_renewal: default_hours_to_renewal(),
            automatic: false,
        }
    }
}

impl ResetAlert {
    pub fn spend_share(&self) -> SpendShare {
        SpendShare(self.max_left_percent.min(SPEND_CAP_PERCENT))
    }

    pub fn keep_within_bounds(&mut self) {
        self.max_left_percent = self
            .max_left_percent
            .clamp(LOWEST_SHARE_PERCENT, SPEND_CAP_PERCENT);
        self.min_hours_to_renewal = self.min_hours_to_renewal.min(MOST_HOURS_TO_RENEWAL);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetSpend {
    ByHand,
    Automatic(SpendShare),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpendShare(u8);

impl SpendShare {
    pub fn allows(self, reading: &Reading) -> Result<(), String> {
        let left = left_percent(reading).ok_or(
            "on-n-off can't tell how much of the Codex limit is left, so it won't spend a reset.",
        )?;
        if left <= f64::from(self.0) {
            return Ok(());
        }
        Err(format!(
            "A banked reset can be used once {}% or less of the current limit is left, and {}% is left.",
            self.0,
            left.round()
        ))
    }
}

fn left_percent(reading: &Reading) -> Option<f64> {
    reading
        .windows
        .iter()
        .filter(|window| {
            matches!(
                window.kind,
                LimitWindowKind::Weekly | LimitWindowKind::Session
            )
        })
        .map(|window| window.used_percent)
        .reduce(f64::max)
        .map(|used| (100.0 - used).max(0.0))
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AlertMemory(HashMap<String, AlertState>);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlertState {
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
struct Offer {
    account_id: String,
    account_label: Option<String>,
    left_percent: f64,
    renews_at: DateTime<Utc>,
    automatic: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Waiting(HashMap<String, WaitingSpend>);

#[derive(Clone, Debug, PartialEq)]
pub struct WaitingSpend {
    account_id: String,
    account_label: Option<String>,
    cycle: DateTime<Utc>,
    due_at: DateTime<Utc>,
    idempotency_key: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub offered: Offered,
    pub due: Vec<Due>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Offered {
    told: Vec<Notice>,
    automatic: Vec<Offer>,
}

#[derive(Debug, PartialEq)]
pub enum Due {
    Spend(WaitingSpend, SpendShare),
    Kept(Notice),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kept {
    NotNeeded,
    Late,
}

pub fn plan(
    memory: &mut AlertMemory,
    waiting: &mut Waiting,
    readings: &[ProviderLimitsDto],
    nobody_signed_in: &[AgentId],
    alerts: &HashMap<String, ResetAlert>,
    now: DateTime<Utc>,
) -> Plan {
    let due = waiting.fall_due(readings, nobody_signed_in, alerts, now);
    let (automatic, told): (Vec<_>, Vec<_>) = memory
        .offers(readings, alerts, now)
        .into_iter()
        .partition(|offer| offer.automatic);
    Plan {
        offered: Offered {
            told: told.iter().map(|offer| offer.notice(now)).collect(),
            automatic,
        },
        due,
    }
}

impl Offered {
    pub fn commit(self, waiting: &mut Waiting, now: DateTime<Utc>) -> Vec<Notice> {
        let mut notices = self.told;
        notices.extend(waiting.schedule(&self.automatic, now));
        notices
    }
}

impl AlertMemory {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn offers(
        &mut self,
        readings: &[ProviderLimitsDto],
        alerts: &HashMap<String, ResetAlert>,
        now: DateTime<Utc>,
    ) -> Vec<Offer> {
        self.0.retain(|account, _| alerts.contains_key(account));
        let mut offers = Vec::new();
        for card in readings {
            let Some(account) = signed_in_codex(card) else {
                continue;
            };
            let Some(alert) = alerts.get(&account.id) else {
                continue;
            };
            let entry = self.0.entry(account.id.clone()).or_default();
            match low_reading(card, alert, now) {
                None => entry.low = None,
                Some((reading, offer)) => {
                    let confirmed = entry.low.as_ref().is_some_and(|low| {
                        saved_cycle_is(&low.cycle, offer.renews_at)
                            && is_newer(&reading.observed_at, &low.observed_at)
                    });
                    let offered = entry
                        .offered_cycle
                        .as_deref()
                        .is_some_and(|offered| saved_cycle_is(offered, offer.renews_at));
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
}

fn signed_in_codex(card: &ProviderLimitsDto) -> Option<&LimitsAccountDto> {
    let account = card.account.as_ref()?;
    (card.provider == AgentId::Codex && card.current_account && card.status == LimitsStatus::Ok)
        .then_some(account)
}

fn low_reading(
    card: &ProviderLimitsDto,
    alert: &ResetAlert,
    now: DateTime<Utc>,
) -> Option<(LowReading, Offer)> {
    let account = signed_in_codex(card)?;
    let left = left_percent(&card.reading)?;
    if left > f64::from(alert.spend_share().0) {
        return None;
    }
    let weekly = card
        .reading
        .windows
        .iter()
        .find(|window| window.kind == LimitWindowKind::Weekly)?;
    let cycle = weekly.resets_at.as_deref()?;
    let renews_at = instant(cycle)?;
    if renews_at - now < chrono::Duration::hours(i64::from(alert.min_hours_to_renewal)) {
        return None;
    }
    let banked = card.reading.reset_credits.as_ref()?;
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
            account_id: account.id.clone(),
            account_label: account.label.clone(),
            left_percent: left,
            renews_at,
            automatic: alert.automatic,
        },
    ))
}

fn instant(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

fn saved_cycle_is(saved: &str, cycle: DateTime<Utc>) -> bool {
    instant(saved).is_some_and(|saved| same_reset_instant(saved, cycle))
}

fn is_newer(later: &str, earlier: &str) -> bool {
    instant(later)
        .zip(instant(earlier))
        .is_some_and(|(later, earlier)| later > earlier)
}

enum CodexNow<'a> {
    Live(&'a ProviderLimitsDto),
    NobodySignedIn,
    Unknown,
}

fn codex_now<'a>(readings: &'a [ProviderLimitsDto], nobody_signed_in: &[AgentId]) -> CodexNow<'a> {
    let current = readings
        .iter()
        .find(|card| card.provider == AgentId::Codex && card.current_account);
    match current {
        Some(card) if signed_in_codex(card).is_some() => CodexNow::Live(card),
        Some(_) if nobody_signed_in.contains(&AgentId::Codex) => CodexNow::NobodySignedIn,
        _ => CodexNow::Unknown,
    }
}

impl Waiting {
    fn fall_due(
        &mut self,
        readings: &[ProviderLimitsDto],
        nobody_signed_in: &[AgentId],
        alerts: &HashMap<String, ResetAlert>,
        now: DateTime<Utc>,
    ) -> Vec<Due> {
        self.0
            .retain(|account, _| alerts.get(account).is_some_and(|alert| alert.automatic));
        let late = |spend: &WaitingSpend| {
            spend.due_at + chrono::Duration::minutes(LATE_AFTER_MINUTES) < now
        };
        let codex = codex_now(readings, nobody_signed_in);
        let decided = !matches!(codex, CodexNow::Unknown);
        self.0
            .extract_if(|_, spend| spend.due_at <= now && (decided || late(spend)))
            .map(|(account, spend)| {
                if late(&spend) {
                    return Due::Kept(spend.kept_notice(Kept::Late));
                }
                let deciding_alert = match codex {
                    CodexNow::Live(card) => alerts.get(&account).filter(|alert| {
                        low_reading(card, alert, now).is_some_and(|(_, offer)| {
                            offer.account_id == account
                                && same_reset_instant(offer.renews_at, spend.cycle)
                        })
                    }),
                    CodexNow::NobodySignedIn | CodexNow::Unknown => None,
                };
                match deciding_alert {
                    Some(alert) => Due::Spend(spend, alert.spend_share()),
                    None => Due::Kept(spend.kept_notice(Kept::NotNeeded)),
                }
            })
            .collect()
    }

    fn schedule(&mut self, offers: &[Offer], now: DateTime<Utc>) -> Vec<Notice> {
        for offer in offers {
            let spend = WaitingSpend {
                account_id: offer.account_id.clone(),
                account_label: offer.account_label.clone(),
                cycle: offer.renews_at,
                due_at: now + chrono::Duration::minutes(SPEND_DELAY_MINUTES),
                idempotency_key: uuid::Uuid::new_v4().to_string(),
            };
            self.0.insert(spend.account_id.clone(), spend);
        }
        offers.iter().map(Offer::scheduled_notice).collect()
    }

    pub fn any_due(&self, now: DateTime<Utc>) -> bool {
        self.0.values().any(|spend| spend.due_at <= now)
    }

    pub fn next_due(&self) -> Option<DateTime<Utc>> {
        self.0.values().map(|spend| spend.due_at).min()
    }

    pub fn listed(&self) -> Vec<PendingResetSpendDto> {
        self.0
            .values()
            .map(|spend| PendingResetSpendDto {
                account_id: spend.account_id.clone(),
                due_at: spend.due_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            })
            .collect()
    }

    pub fn cancel(&mut self, account_id: &str) -> bool {
        self.0.remove(account_id).is_some()
    }

    pub fn used_by_hand(&mut self, account_id: &str, outcome: ResetCreditOutcome) {
        if outcome == ResetCreditOutcome::Reset {
            self.cancel(account_id);
        }
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

impl WaitingSpend {
    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
}

mod copy;

#[cfg(test)]
mod tests;
