use crate::dto::{AgentId, Reading};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) label: String,
    pub(crate) capacity: Option<u32>,
}

struct Tier {
    provider: AgentId,
    reported: &'static str,
    label: &'static str,
    capacity: u32,
    claude_rate_limit_tier: Option<&'static str>,
}

const CLAUDE_MAX: &str = "max";

const TIERS: [Tier; 5] = [
    Tier {
        provider: AgentId::Claude,
        reported: "max ×20",
        label: "Max ×20",
        capacity: 20,
        claude_rate_limit_tier: Some("default_claude_max_20x"),
    },
    Tier {
        provider: AgentId::Claude,
        reported: "max ×5",
        label: "Max ×5",
        capacity: 5,
        claude_rate_limit_tier: Some("default_claude_max_5x"),
    },
    Tier {
        provider: AgentId::Claude,
        reported: CLAUDE_MAX,
        label: "Max",
        capacity: 5,
        claude_rate_limit_tier: None,
    },
    Tier {
        provider: AgentId::Codex,
        reported: "pro",
        label: "Pro ×20",
        capacity: 20,
        claude_rate_limit_tier: None,
    },
    Tier {
        provider: AgentId::Codex,
        reported: "prolite",
        label: "Pro ×5",
        capacity: 5,
        claude_rate_limit_tier: None,
    },
];

const CODEX_WORKSPACE_PLANS: [&str; 11] = [
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
];

pub(crate) fn describe(provider: AgentId, reported: &str) -> Option<Plan> {
    let reported = reported.trim();
    if reported.is_empty() {
        return None;
    }
    let key = tier_key(reported);
    let tier = TIERS
        .iter()
        .find(|tier| tier.provider == provider && tier_key(tier.reported) == key);
    Some(match tier {
        Some(tier) => Plan {
            label: tier.label.to_string(),
            capacity: Some(tier.capacity),
        },
        None => Plan {
            label: spelled(reported),
            capacity: None,
        },
    })
}

pub(crate) fn describe_reading(provider: AgentId, reading: &mut Reading) {
    let plan = reading
        .plan
        .as_deref()
        .and_then(|reported| describe(provider, reported));
    reading.plan_capacity = plan.as_ref().and_then(|plan| plan.capacity);
    reading.plan_label = plan.map(|plan| plan.label);
}

pub(crate) fn claude_plan(
    organization_type: Option<&str>,
    rate_limit_tier: Option<&str>,
) -> Option<String> {
    let tier = TIERS.iter().find(|tier| {
        tier.provider == AgentId::Claude
            && organization_type == Some(CLAUDE_MAX)
            && tier.claude_rate_limit_tier.is_some()
            && tier.claude_rate_limit_tier == rate_limit_tier
    });
    tier.map(|tier| tier.reported)
        .or(organization_type)
        .map(str::to_string)
}

pub(crate) fn is_codex_workspace_plan(plan: &str) -> bool {
    CODEX_WORKSPACE_PLANS.contains(&plan)
}

fn tier_key(plan: &str) -> String {
    plan.trim()
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .collect()
}

fn spelled(reported: &str) -> String {
    let spaced = reported.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests;
