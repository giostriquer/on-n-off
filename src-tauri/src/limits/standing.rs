use crate::dto::{
    LastObserved, LimitsCardDto, LimitsStatus, MessageTone, ProviderLimitsDto, ReadStanding,
    StandingMessage,
};

impl ReadStanding {
    pub(crate) fn of(card: &ProviderLimitsDto) -> Self {
        let observed = card.reading.has_observations();
        let remembered = !card.current_account && !card.saved_profile;
        if card.status == LimitsStatus::Ok {
            return if observed && remembered {
                Self::LastObserved(LastObserved::Remembered)
            } else {
                Self::Live
            };
        }
        let message = StandingMessage::of(card);
        if !observed {
            return Self::Unavailable { message };
        }
        Self::LastObserved(if card.saved_profile {
            LastObserved::SavedRefreshPaused { message }
        } else if remembered {
            LastObserved::Remembered
        } else {
            LastObserved::ReadPaused { message }
        })
    }
}

impl StandingMessage {
    fn of(card: &ProviderLimitsDto) -> Self {
        Self {
            text: card
                .message
                .clone()
                .unwrap_or_else(|| super::outcome::unexplained(card.provider)),
            tone: if card.status == LimitsStatus::Failed {
                MessageTone::Error
            } else {
                MessageTone::Muted
            },
        }
    }
}

impl From<ProviderLimitsDto> for LimitsCardDto {
    fn from(card: ProviderLimitsDto) -> Self {
        Self {
            standing: ReadStanding::of(&card),
            card,
        }
    }
}

#[cfg(test)]
pub(crate) mod cases;
#[cfg(test)]
mod tests;
