//! First usage observation during isolated official sign-in, before its native home is removed.
//! No saved vault credential is loaded or independently renewed here.
use super::{credentials::ClaudeCredential, *};
use crate::accounts::model::Identity;

pub(crate) fn read(
    isolated_home: &Path,
    identity: &Identity,
    claude: Option<ClaudeCredential>,
) -> Option<ProviderLimitsDto> {
    read_with(
        isolated_home,
        identity,
        claude,
        CLAUDE_PROFILE_URL,
        CLAUDE_USAGE_URL,
    )
}

fn read_with(
    isolated_home: &Path,
    identity: &Identity,
    claude: Option<ClaudeCredential>,
    profile_url: &str,
    usage_url: &str,
) -> Option<ProviderLimitsDto> {
    let mut dto = match identity.provider {
        AgentId::Codex => codex_limits(isolated_home, false),
        AgentId::Claude => {
            let selected = Some(ClaudeIdentity {
                account: LimitsAccountDto {
                    id: identity.user_id.clone(),
                    label: None,
                    legacy_id: None,
                },
                organization_id: Some(identity.workspace_id.clone()),
            });
            let mut dto = claude_limits(
                CredentialLookup::Found(claude?),
                &selected,
                profile_url,
                usage_url,
            )
            .dto;
            if dto.status != LimitsStatus::Ok {
                return None;
            }
            let account = dto.account.as_mut()?;
            account.legacy_id = Some(identity.user_id.clone());
            account.id = identity.observation_key();
            dto
        }
        _ => return None,
    };
    if dto.status != LimitsStatus::Ok
        || dto.account.as_ref()?.id != identity.observation_key()
        || (dto.windows.is_empty() && dto.credits.is_none())
    {
        return None;
    }
    dto.current_account = false;
    Some(dto)
}

/// Called only after the account registry accepted this sign-in's identity and operation epoch.
pub(crate) fn remember(home: &Path, usage: &ProviderLimitsDto) -> Result<(), String> {
    SnapshotStore::for_home(home).save(usage)
}

#[cfg(test)]
mod tests;
