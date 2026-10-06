use std::path::Path;

use serde_json::Value;

use super::json::optional_string;
use crate::accounts::model::Identity;
use crate::dto::LimitsAccountDto;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudeIdentity {
    pub(crate) account: LimitsAccountDto,
    pub(crate) organization_id: Option<String>,
}

fn plan_label(subscription_type: Option<&str>, tier: Option<&str>) -> Option<String> {
    match (subscription_type, tier) {
        (Some("max"), Some("default_claude_max_5x")) => Some("max ×5".to_string()),
        (Some("max"), Some("default_claude_max_20x")) => Some("max ×20".to_string()),
        _ => subscription_type.map(str::to_string),
    }
}

pub(super) struct ClaudeConfigAccount {
    pub(super) identity: ClaudeIdentity,
    pub(super) plan: Option<String>,
}

pub(super) fn read_claude_config_account(config_file: &Path) -> Option<ClaudeConfigAccount> {
    let value = read_json_file(config_file).ok()??;
    let account = value.get("oauthAccount")?;
    let organization_type = optional_string(account.get("organizationType"));
    let tier = optional_string(account.get("organizationRateLimitTier"));
    Some(ClaudeConfigAccount {
        identity: ClaudeIdentity {
            account: LimitsAccountDto {
                legacy_id: None,
                id: optional_string(account.get("accountUuid"))?,
                label: optional_string(account.get("emailAddress")),
            },
            organization_id: optional_string(account.get("organizationUuid")),
        },
        plan: plan_label(
            organization_type
                .as_deref()
                .and_then(|kind| kind.strip_prefix("claude_")),
            tier.as_deref(),
        ),
    })
}

impl ClaudeIdentity {
    pub(super) fn account_key(&self) -> (&str, Option<&str>) {
        (&self.account.id, self.organization_id.as_deref())
    }

    pub(super) fn names(&self, identity: &Identity) -> bool {
        self.account.id == identity.user_id
            && self.organization_id.as_deref() == Some(identity.workspace_id.as_str())
    }
}

fn read_json_file(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<Value>(&raw)
            .map(Some)
            .map_err(|error| format!("{}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests;
