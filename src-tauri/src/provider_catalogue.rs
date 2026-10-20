use std::path::Path;

use crate::dto::AgentId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderFacts {
    pub key: &'static str,
    pub display_name: &'static str,
    pub binary: &'static str,
    pub binary_aliases: &'static [&'static str],
    pub limits: Option<LimitsFacts>,
    pub usage: Option<UsageProvider>,
    pub saved_accounts: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitsFacts {
    pub inherits_legacy_reading: bool,
    pub on_load: &'static [OnLoad],
    pub credits_spent: Option<CreditsSpent>,
    pub session_rollouts: Option<SessionRollouts>,
    pub banked_resets: Option<BankedResets>,
    pub sign_in_deadline: Option<SignInDeadline>,
    pub subscription_term: Option<SubscriptionTerm>,
    pub paid_reset_offer: Option<PaidResetOffer>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnLoad {
    DropHiddenWindows,
    DropResetCredits,
    MarkFirstMessageSessions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CreditsSpent {
    Codex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionRollouts {
    Codex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BankedResets {
    Codex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SignInDeadline {
    Claude,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionTerm {
    Codex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PaidResetOffer {
    Codex,
}

impl LimitsFacts {
    pub const NONE: Self = Self {
        inherits_legacy_reading: false,
        on_load: &[],
        credits_spent: None,
        session_rollouts: None,
        banked_resets: None,
        sign_in_deadline: None,
        subscription_term: None,
        paid_reset_offer: None,
    };
}

impl AgentId {
    pub const ALL: [Self; 5] = [
        Self::Claude,
        Self::Codex,
        Self::Antigravity,
        Self::Cursor,
        Self::Muse,
    ];

    pub const fn facts(self) -> ProviderFacts {
        match self {
            Self::Claude => ProviderFacts {
                key: "claude",
                display_name: "Claude",
                binary: "claude",
                binary_aliases: &[],
                limits: Some(LimitsFacts {
                    on_load: &[OnLoad::DropResetCredits, OnLoad::MarkFirstMessageSessions],
                    sign_in_deadline: Some(SignInDeadline::Claude),
                    ..LimitsFacts::NONE
                }),
                usage: Some(UsageProvider::Claude),
                saved_accounts: true,
            },
            Self::Codex => ProviderFacts {
                key: "codex",
                display_name: "Codex",
                binary: "codex",
                binary_aliases: &[],
                limits: Some(LimitsFacts {
                    on_load: &[OnLoad::DropHiddenWindows],
                    credits_spent: Some(CreditsSpent::Codex),
                    session_rollouts: Some(SessionRollouts::Codex),
                    banked_resets: Some(BankedResets::Codex),
                    subscription_term: Some(SubscriptionTerm::Codex),
                    paid_reset_offer: Some(PaidResetOffer::Codex),
                    ..LimitsFacts::NONE
                }),
                usage: Some(UsageProvider::Codex),
                saved_accounts: true,
            },
            Self::Antigravity => ProviderFacts {
                key: "antigravity",
                display_name: "Antigravity",
                binary: "agy",
                binary_aliases: &[],
                limits: None,
                usage: None,
                saved_accounts: false,
            },
            Self::Cursor => ProviderFacts {
                key: "cursor",
                display_name: "Cursor",
                binary: "agent",
                binary_aliases: &["cursor-agent"],
                limits: None,
                usage: None,
                saved_accounts: false,
            },
            Self::Muse => ProviderFacts {
                key: "muse",
                display_name: "Muse",
                binary: "muse",
                binary_aliases: &[],
                limits: Some(LimitsFacts {
                    inherits_legacy_reading: true,
                    on_load: &[OnLoad::MarkFirstMessageSessions],
                    ..LimitsFacts::NONE
                }),
                usage: Some(UsageProvider::Muse),
                saved_accounts: true,
            },
        }
    }

    pub const fn key(self) -> &'static str {
        self.facts().key
    }

    pub const fn limits_facts(self) -> LimitsFacts {
        match self.facts().limits {
            Some(facts) => facts,
            None => LimitsFacts::NONE,
        }
    }

    pub const fn display_name(self) -> &'static str {
        self.facts().display_name
    }

    pub const fn binary_name(self) -> &'static str {
        self.facts().binary
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|id| id.key() == key)
    }

    pub fn from_binary(cli_name: &str) -> Option<Self> {
        let name = Path::new(cli_name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(cli_name)
            .to_ascii_lowercase();
        Self::ALL.into_iter().find(|id| {
            let facts = id.facts();
            facts.binary == name || facts.binary_aliases.contains(&name.as_str())
        })
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum UsageProvider {
    Claude,
    Codex,
    Muse,
}

impl UsageProvider {
    pub fn agent(self) -> AgentId {
        match self {
            Self::Claude => AgentId::Claude,
            Self::Codex => AgentId::Codex,
            Self::Muse => AgentId::Muse,
        }
    }

    pub fn as_str(self) -> &'static str {
        self.agent().key()
    }

    pub fn from_key(key: &str) -> Option<Self> {
        AgentId::from_key(key)?.facts().usage
    }

    pub fn all() -> impl Iterator<Item = Self> {
        AgentId::ALL.into_iter().filter_map(|id| id.facts().usage)
    }
}

pub const RAIL_ORDER: [AgentId; AgentId::ALL.len()] = [
    AgentId::Claude,
    AgentId::Codex,
    AgentId::Muse,
    AgentId::Antigravity,
    AgentId::Cursor,
];

const _: () = assert!(
    lists_every_provider(&RAIL_ORDER),
    "the rail lists every provider once"
);

pub const WITH_LIMITS: [AgentId; Fact::LimitsReader.count()] = Fact::LimitsReader.providers();

pub const WITH_SAVED_ACCOUNTS: [AgentId; Fact::SavedAccounts.count()] =
    Fact::SavedAccounts.providers();

pub const WITH_BANKED_RESETS: [AgentId; Fact::BankedResets.count()] =
    Fact::BankedResets.providers();

const _: () = assert!(
    Fact::BankedResets.count() == 1,
    "one provider banks resets: the reset alert, its spend and its re-read follow that one provider"
);

pub const BANKED_RESETS_PROVIDER: AgentId = WITH_BANKED_RESETS[0];

const fn lists_every_provider(order: &[AgentId; AgentId::ALL.len()]) -> bool {
    let mut i = 0;
    while i < AgentId::ALL.len() {
        let mut listed = false;
        let mut j = 0;
        while j < order.len() {
            listed |= order[j] as u8 == AgentId::ALL[i] as u8;
            j += 1;
        }
        if !listed {
            return false;
        }
        i += 1;
    }
    true
}

#[derive(Clone, Copy)]
enum Fact {
    LimitsReader,
    SavedAccounts,
    BankedResets,
}

impl Fact {
    const fn holds_for(self, id: AgentId) -> bool {
        let facts = id.facts();
        match self {
            Self::LimitsReader => facts.limits.is_some(),
            Self::SavedAccounts => facts.saved_accounts,
            Self::BankedResets => id.limits_facts().banked_resets.is_some(),
        }
    }

    const fn count(self) -> usize {
        let mut count = 0;
        let mut i = 0;
        while i < AgentId::ALL.len() {
            if self.holds_for(AgentId::ALL[i]) {
                count += 1;
            }
            i += 1;
        }
        count
    }

    const fn providers<const N: usize>(self) -> [AgentId; N] {
        assert!(
            N == self.count(),
            "the list length must be the number of providers with the fact"
        );
        let mut providers = [AgentId::Claude; N];
        let mut filled = 0;
        let mut i = 0;
        while i < AgentId::ALL.len() {
            if self.holds_for(AgentId::ALL[i]) {
                providers[filled] = AgentId::ALL[i];
                filled += 1;
            }
            i += 1;
        }
        providers
    }
}

#[cfg(test)]
mod tests;
