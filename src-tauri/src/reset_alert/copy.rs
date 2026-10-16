use chrono::{DateTime, Utc};

use super::{Kept, Notice, Offer, WaitingSpend, SPEND_DELAY_MINUTES};
use crate::dto::ResetCreditOutcome;

impl Offer {
    pub(super) fn notice(&self, now: DateTime<Utc>) -> Notice {
        let account = self
            .account_label
            .as_deref()
            .map(|label| format!("{label} has "))
            .unwrap_or_else(|| "Codex has ".to_string());
        let hours = (self.renews_at - now).num_hours().max(0);
        let renews = if hours >= 24 {
            format!("{}d {}h", hours / 24, hours % 24)
        } else {
            format!("{hours}h")
        };
        (
            "Codex: a banked reset is available".to_string(),
            format!(
                "{account}{:.0}% of its limit left and renews in {renews}. Open on-n-off to use a banked reset.",
                self.left_percent
            ),
        )
    }

    pub(super) fn scheduled_notice(&self) -> Notice {
        (
            format!("Codex: using a banked reset in {SPEND_DELAY_MINUTES} minutes"),
            format!(
                "{} has {:.0}% of its limit left. Cancel on the account's card in on-n-off to keep the reset.",
                name(self.account_label.as_deref()),
                self.left_percent
            ),
        )
    }
}

impl WaitingSpend {
    pub fn kept_notice(&self, why: Kept) -> Notice {
        let why = match why {
            Kept::NotNeeded => "no longer needs it, or is no longer signed in",
            Kept::Late => "could not be checked in time",
        };
        (
            "Codex: banked reset not used".to_string(),
            format!(
                "{} {why}, so it was kept.",
                name(self.account_label.as_deref())
            ),
        )
    }

    pub fn outcome_notice(&self, outcome: &Result<ResetCreditOutcome, String>) -> Notice {
        let account = name(self.account_label.as_deref());
        let not_used = |why: String| ("Codex: banked reset not used".to_string(), why);
        match outcome {
            Ok(ResetCreditOutcome::Reset) => (
                "Codex: banked reset used".to_string(),
                format!("{account}'s usage is back to 0%."),
            ),
            Ok(ResetCreditOutcome::NothingToReset) => {
                not_used(format!("{account}'s usage was already at 0%."))
            }
            Ok(ResetCreditOutcome::NoCredit) => {
                not_used(format!("{account} has no banked reset left."))
            }
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
}

fn name(label: Option<&str>) -> &str {
    label.unwrap_or("The Codex account")
}
