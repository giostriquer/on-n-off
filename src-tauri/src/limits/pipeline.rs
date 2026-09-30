//! Provider-neutral card assembly: every read becomes a card through [`finish`].

use chrono::{SecondsFormat, Utc};

use super::Parsed;
use crate::dto::{AgentId, LimitWindowKind, LimitsStatus, ProviderLimitsDto};

pub(super) fn finish(
    provider: AgentId,
    status: LimitsStatus,
    message: Option<String>,
    mut parsed: Parsed,
) -> ProviderLimitsDto {
    // Every read becomes a card here, so its windows take the canonical order here, whatever
    // order its endpoint answered in.
    let windows = &mut parsed.reading.windows;
    windows.sort_by_key(|window| kind_rank(window.kind));
    let observed_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    for window in windows {
        if window.observed_at.is_empty() {
            window.observed_at.clone_from(&observed_at);
        }
    }
    ProviderLimitsDto {
        provider,
        status,
        message,
        account: parsed.account,
        current_account: true,
        saved_profile: false,
        archived: false,
        reading: parsed.reading,
    }
}

/// The canonical window order every surface shows: weekly, then session, then per model, windows
/// of one kind in the order their provider gave them. A card's windows take it where they are
/// produced ([`finish`]) and wherever the remember policy adds remembered ones (`reading.rs`).
pub(super) fn kind_rank(kind: LimitWindowKind) -> u8 {
    match kind {
        LimitWindowKind::Weekly => 0,
        LimitWindowKind::Session => 1,
        LimitWindowKind::Model => 2,
    }
}

#[cfg(test)]
mod tests;
