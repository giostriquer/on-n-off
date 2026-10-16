use super::*;
use serde::de::{value::Error, DeserializeOwned, IntoDeserializer};

fn declared_variants<T: DeserializeOwned>() -> Vec<T> {
    (0u32..)
        .map_while(|index| T::deserialize(IntoDeserializer::<Error>::into_deserializer(index)).ok())
        .collect()
}

#[test]
fn the_catalogue_holds_one_row_per_provider_in_tab_order() {
    let rows: Vec<(AgentId, ProviderFacts)> = AgentId::ALL
        .into_iter()
        .map(|id| (id, id.facts()))
        .collect();
    assert_eq!(
        rows,
        [
            (
                AgentId::Claude,
                ProviderFacts {
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
            ),
            (
                AgentId::Codex,
                ProviderFacts {
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
            ),
            (
                AgentId::Antigravity,
                ProviderFacts {
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
            ),
            (
                AgentId::Cursor,
                ProviderFacts {
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
            ),
            (
                AgentId::Muse,
                ProviderFacts {
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
            ),
        ]
    );
}

#[test]
fn every_declared_provider_is_in_the_catalogue_once() {
    let declared: Vec<AgentId> = declared_variants();
    assert_eq!(declared.len(), AgentId::ALL.len(), "{declared:?}");
    for id in declared {
        assert_eq!(
            AgentId::ALL.iter().filter(|listed| **listed == id).count(),
            1,
            "{id:?}"
        );
    }
}

#[test]
fn a_key_is_the_name_settings_and_the_app_write_and_reads_back_as_its_provider() {
    for id in AgentId::ALL {
        assert_eq!(
            serde_json::to_value(id).unwrap(),
            serde_json::json!(id.key())
        );
        assert_eq!(AgentId::from_key(id.key()), Some(id));
    }
    for unknown in ["gemini", "Claude", "", "claude "] {
        assert_eq!(AgentId::from_key(unknown), None, "{unknown:?}");
    }
}

#[test]
fn cursor_uses_agent_as_its_command_and_keeps_the_legacy_alias() {
    assert_eq!(AgentId::Cursor.binary_name(), "agent");
    assert_eq!(AgentId::from_binary("agent"), Some(AgentId::Cursor));
    assert_eq!(AgentId::from_binary("cursor-agent"), Some(AgentId::Cursor));
    assert_eq!(
        AgentId::from_binary("cursor-agent.cmd"),
        Some(AgentId::Cursor)
    );
    assert_eq!(
        AgentId::from_binary("cursor"),
        None,
        "the editor launcher is not the CLI"
    );
}

#[test]
fn every_provider_binary_finds_its_provider_whatever_its_folder_case_or_extension() {
    for (binary, agent) in [
        ("claude", AgentId::Claude),
        ("codex", AgentId::Codex),
        ("agy", AgentId::Antigravity),
        ("agent", AgentId::Cursor),
        ("cursor-agent", AgentId::Cursor),
        ("muse", AgentId::Muse),
        ("/opt/acme/bin/Codex", AgentId::Codex),
        ("CLAUDE.exe", AgentId::Claude),
        ("agy.cmd", AgentId::Antigravity),
        ("muse.cmd", AgentId::Muse),
    ] {
        assert_eq!(AgentId::from_binary(binary), Some(agent), "{binary}");
    }
    for unknown in ["gemini", "", "claude-code", "/opt/acme/bin/"] {
        assert_eq!(AgentId::from_binary(unknown), None, "{unknown}");
    }
}

#[test]
fn every_provider_binary_and_alias_finds_its_own_provider() {
    for id in AgentId::ALL {
        let facts = id.facts();
        for name in std::iter::once(facts.binary).chain(facts.binary_aliases.iter().copied()) {
            assert_eq!(AgentId::from_binary(name), Some(id), "{name}");
        }
    }
}

#[test]
fn the_rail_leads_with_the_providers_that_have_limits_to_draw() {
    assert_eq!(
        RAIL_ORDER,
        [
            AgentId::Claude,
            AgentId::Codex,
            AgentId::Muse,
            AgentId::Antigravity,
            AgentId::Cursor,
        ]
    );
    for (rank, id) in RAIL_ORDER.into_iter().enumerate() {
        assert_eq!(id.facts().rail_rank, rank, "{id:?}");
    }
}

#[test]
fn each_fact_list_holds_exactly_the_providers_with_that_fact_in_tab_order() {
    let with = |fact: fn(&ProviderFacts) -> bool| -> Vec<AgentId> {
        AgentId::ALL
            .into_iter()
            .filter(|id| fact(&id.facts()))
            .collect()
    };
    assert_eq!(WITH_LIMITS.to_vec(), with(|facts| facts.limits_reader));
    assert_eq!(
        WITH_SAVED_ACCOUNTS.to_vec(),
        with(|facts| facts.saved_accounts)
    );
}

#[test]
fn a_provider_usage_reads_is_the_one_its_transcripts_and_folded_rows_name() {
    for id in AgentId::ALL {
        match id.facts().usage {
            Some(usage) => {
                assert_eq!(usage.agent(), id);
                assert_eq!(usage.as_str(), id.key());
                assert_eq!(
                    serde_json::to_value(usage).unwrap(),
                    serde_json::json!(id.key())
                );
                assert_eq!(UsageProvider::from_key(id.key()), Some(usage));
            }
            None => assert_eq!(UsageProvider::from_key(id.key()), None, "{id:?}"),
        }
    }
    let declared: Vec<UsageProvider> = declared_variants();
    assert_eq!(declared.len(), UsageProvider::all().count(), "{declared:?}");
    for usage in declared {
        assert_eq!(usage.agent().facts().usage, Some(usage), "{usage:?}");
    }
    assert_eq!(UsageProvider::from_key("gemini"), None);
}
