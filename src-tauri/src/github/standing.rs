use crate::dto::{
    GithubPrsDto, GithubPrsReplyDto, GithubStatus, PrMessageTone, PrReadStanding, PrStandingMessage,
};

const UNEXPLAINED: &str = "GitHub did not answer.";

impl PrReadStanding {
    pub(crate) fn of(read: &GithubPrsDto) -> Self {
        let Some(tone) = problem_tone(read.status) else {
            return Self::Live;
        };
        let message = PrStandingMessage {
            text: read.hint.clone().unwrap_or_else(|| UNEXPLAINED.into()),
            tone,
        };
        if read.stale {
            Self::LastObserved { message }
        } else {
            Self::Unavailable { message }
        }
    }

    pub(crate) fn sent_with(read: &GithubPrsDto) -> Option<Self> {
        Some(Self::of(read)).filter(|standing| *standing != Self::Live)
    }
}

fn problem_tone(status: GithubStatus) -> Option<PrMessageTone> {
    match status {
        GithubStatus::Ok => None,
        GithubStatus::GhMissing | GithubStatus::GhNotLoggedIn | GithubStatus::TokenRejected => {
            Some(PrMessageTone::Error)
        }
        GithubStatus::RateLimited | GithubStatus::Network => Some(PrMessageTone::Warning),
    }
}

impl GithubPrsReplyDto {
    pub(crate) fn of(read: GithubPrsDto) -> Self {
        Self {
            standing: PrReadStanding::sent_with(&read),
            read,
        }
    }
}

#[cfg(test)]
pub(crate) mod cases;
#[cfg(test)]
mod tests;
