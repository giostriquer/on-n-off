use std::path::Path;

use crate::dto::AgentId;
use crate::usage::UsageProvider;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderFacts {
    pub key: &'static str,
    pub display_name: &'static str,
    pub binary: &'static str,
    pub binary_aliases: &'static [&'static str],
    pub rail_rank: usize,
    pub on_default_notch: bool,
    pub limits_reader: bool,
    pub usage: Option<UsageProvider>,
    pub saved_accounts: bool,
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
                rail_rank: 0,
                on_default_notch: true,
                limits_reader: true,
                usage: Some(UsageProvider::Claude),
                saved_accounts: true,
            },
            Self::Codex => ProviderFacts {
                key: "codex",
                display_name: "Codex",
                binary: "codex",
                binary_aliases: &[],
                rail_rank: 1,
                on_default_notch: true,
                limits_reader: true,
                usage: Some(UsageProvider::Codex),
                saved_accounts: true,
            },
            Self::Antigravity => ProviderFacts {
                key: "antigravity",
                display_name: "Antigravity",
                binary: "agy",
                binary_aliases: &[],
                rail_rank: 3,
                on_default_notch: false,
                limits_reader: false,
                usage: None,
                saved_accounts: false,
            },
            Self::Cursor => ProviderFacts {
                key: "cursor",
                display_name: "Cursor",
                binary: "agent",
                binary_aliases: &["cursor-agent"],
                rail_rank: 4,
                on_default_notch: false,
                limits_reader: false,
                usage: None,
                saved_accounts: false,
            },
            Self::Muse => ProviderFacts {
                key: "muse",
                display_name: "Muse",
                binary: "muse",
                binary_aliases: &[],
                rail_rank: 2,
                on_default_notch: true,
                limits_reader: true,
                usage: Some(UsageProvider::Muse),
                saved_accounts: false,
            },
        }
    }

    pub const fn key(self) -> &'static str {
        self.facts().key
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

pub const RAIL_ORDER: [AgentId; AgentId::ALL.len()] = rail_order();

pub const WITH_LIMITS: [AgentId; Fact::LimitsReader.count()] = Fact::LimitsReader.providers();

pub const WITH_SAVED_ACCOUNTS: [AgentId; Fact::SavedAccounts.count()] =
    Fact::SavedAccounts.providers();

const fn rail_order() -> [AgentId; AgentId::ALL.len()] {
    let mut order = AgentId::ALL;
    let mut taken = [false; AgentId::ALL.len()];
    let mut i = 0;
    while i < AgentId::ALL.len() {
        let rank = AgentId::ALL[i].facts().rail_rank;
        assert!(
            rank < taken.len() && !taken[rank],
            "every provider needs its own rail rank below the provider count"
        );
        taken[rank] = true;
        order[rank] = AgentId::ALL[i];
        i += 1;
    }
    order
}

#[derive(Clone, Copy)]
enum Fact {
    LimitsReader,
    SavedAccounts,
}

impl Fact {
    const fn holds_for(self, id: AgentId) -> bool {
        let facts = id.facts();
        match self {
            Self::LimitsReader => facts.limits_reader,
            Self::SavedAccounts => facts.saved_accounts,
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
