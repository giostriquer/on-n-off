use crate::dto::{
    LastObservedWhy, LimitsStatus, MessageTone, ProviderLimitsDto, ReadStanding, StandingMessage,
};

pub(crate) fn stand(cards: &mut [ProviderLimitsDto]) {
    for card in cards {
        card.standing = ReadStanding::of(card);
    }
}

impl ReadStanding {
    pub(crate) fn of(card: &ProviderLimitsDto) -> Self {
        let message = (card.status != LimitsStatus::Ok).then(|| StandingMessage {
            text: card.message.clone().unwrap_or_else(|| {
                format!("{} limits are unavailable.", card.provider.display_name())
            }),
            tone: if card.status == LimitsStatus::Failed {
                MessageTone::Error
            } else {
                MessageTone::Muted
            },
        });
        if !card.reading.has_observations() {
            return message.map_or(Self::Live, |message| Self::Unavailable { message });
        }
        let why = if card.saved_profile && message.is_some() {
            LastObservedWhy::SavedRefreshPaused
        } else if !card.current_account && !card.saved_profile {
            LastObservedWhy::Remembered
        } else {
            match card.status {
                LimitsStatus::Ok => return Self::Live,
                LimitsStatus::Failed => LastObservedWhy::ReadFailed,
                LimitsStatus::SignedOut
                | LimitsStatus::Unauthenticated
                | LimitsStatus::Unsupported => LastObservedWhy::SignedOut,
            }
        };
        Self::LastObserved { why, message }
    }
}

#[cfg(test)]
pub(crate) mod cases;
#[cfg(test)]
mod tests;
