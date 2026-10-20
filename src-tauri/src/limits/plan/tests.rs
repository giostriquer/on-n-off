use super::*;

fn shown(provider: AgentId, reported: &str) -> Option<(String, Option<u32>)> {
    describe(provider, reported).map(|plan| (plan.label, plan.capacity))
}

#[test]
fn every_plan_reads_as_the_label_the_card_shows_and_the_capacity_it_orders_by() {
    let rows: [(AgentId, &str, &str, Option<u32>); 24] = [
        (AgentId::Claude, "max ×20", "Max ×20", Some(20)),
        (AgentId::Claude, "max ×5", "Max ×5", Some(5)),
        (AgentId::Claude, "max", "Max", Some(5)),
        (AgentId::Claude, "pro", "Pro", None),
        (AgentId::Claude, "team", "Team", None),
        (AgentId::Claude, "enterprise", "Enterprise", None),
        (AgentId::Claude, "enterprise_x", "Enterprise x", None),
        (AgentId::Codex, "pro", "Pro ×20", Some(20)),
        (AgentId::Codex, "PRO", "Pro ×20", Some(20)),
        (AgentId::Codex, "prolite", "Pro ×5", Some(5)),
        (AgentId::Codex, "pro_lite", "Pro ×5", Some(5)),
        (AgentId::Codex, "Pro Lite", "Pro ×5", Some(5)),
        (AgentId::Codex, "pro-lite", "Pro ×5", Some(5)),
        (AgentId::Codex, " Pro Lite ", "Pro ×5", Some(5)),
        (AgentId::Codex, "plus", "Plus", None),
        (AgentId::Codex, "free", "Free", None),
        (AgentId::Codex, "team", "Team", None),
        (AgentId::Codex, "business", "Business", None),
        (
            AgentId::Codex,
            "self_serve_business_prolite",
            "Self serve business prolite",
            None,
        ),
        (AgentId::Codex, "future_plan", "Future plan", None),
        (AgentId::Muse, "Wonder", "Wonder", None),
        (AgentId::Muse, "wonder", "Wonder", None),
        (
            AgentId::Muse,
            "Muse Code Power Usage",
            "Muse Code Power Usage",
            None,
        ),
        (AgentId::Muse, "  pro  ", "Pro", None),
    ];
    for (provider, reported, label, capacity) in rows {
        assert_eq!(
            shown(provider, reported),
            Some((label.to_string(), capacity)),
            "{provider:?} {reported:?}"
        );
    }
}

#[test]
fn a_tier_is_its_own_providers_and_no_other_providers_plan_borrows_it() {
    assert_eq!(shown(AgentId::Claude, "pro"), Some(("Pro".into(), None)));
    assert_eq!(
        shown(AgentId::Muse, "prolite"),
        Some(("Prolite".into(), None))
    );
    assert_eq!(shown(AgentId::Codex, "max"), Some(("Max".into(), None)));
    assert_eq!(
        shown(AgentId::Codex, "max ×20"),
        Some(("Max ×20".into(), None))
    );
}

#[test]
fn no_plan_and_a_blank_one_describe_nothing() {
    for blank in ["", "   "] {
        for provider in crate::provider_catalogue::WITH_LIMITS {
            assert_eq!(describe(provider, blank), None, "{provider:?} {blank:?}");
        }
    }
    let mut reading = Reading {
        plan_label: Some("Stale".into()),
        plan_capacity: Some(9),
        ..Reading::default()
    };
    describe_reading(AgentId::Codex, &mut reading);
    assert_eq!((reading.plan_label, reading.plan_capacity), (None, None));
}

#[test]
fn a_reading_carries_its_plans_label_and_capacity_beside_the_plan_as_reported() {
    let mut reading = Reading {
        plan: Some("prolite".into()),
        ..Reading::default()
    };
    describe_reading(AgentId::Codex, &mut reading);
    assert_eq!(reading.plan.as_deref(), Some("prolite"));
    assert_eq!(reading.plan_label.as_deref(), Some("Pro ×5"));
    assert_eq!(reading.plan_capacity, Some(5));
    reading.plan = Some("plus".into());
    describe_reading(AgentId::Codex, &mut reading);
    assert_eq!(reading.plan_label.as_deref(), Some("Plus"));
    assert_eq!(reading.plan_capacity, None);
}

#[test]
fn claudes_plan_is_its_organization_type_at_its_rate_limit_tier() {
    let rows = [
        (Some("max"), Some("default_claude_max_5x"), Some("max ×5")),
        (Some("max"), Some("default_claude_max_20x"), Some("max ×20")),
        (Some("max"), Some("default_claude_ai"), Some("max")),
        (Some("max"), None, Some("max")),
        (Some("pro"), Some("default_claude_max_20x"), Some("pro")),
        (Some("pro"), Some("default_claude_ai"), Some("pro")),
        (None, Some("default_claude_max_5x"), None),
    ];
    for (organization_type, tier, plan) in rows {
        assert_eq!(
            claude_plan(organization_type, tier).as_deref(),
            plan,
            "{organization_type:?} {tier:?}"
        );
    }
}

#[test]
fn workspace_plans_are_the_ones_codex_counts_as_workspace_accounts() {
    for plan in [
        "team",
        "self_serve_business_prolite",
        "self_serve_business_usage_based",
        "business",
        "ent26",
        "enterprise_cbp_automation",
        "enterprise_cbp_usage_based",
        "enterprise",
        "edu",
        "edu_plus",
        "edu_pro",
    ] {
        assert!(is_codex_workspace_plan(plan), "{plan}");
    }
    for plan in [
        "free", "go", "plus", "pro", "prolite", "guest", "", "unknown",
    ] {
        assert!(!is_codex_workspace_plan(plan), "{plan}");
    }
}
