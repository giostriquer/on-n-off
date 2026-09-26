//! Access-only saved-account reads. No CLI is started and no credential is renewed here.
use super::*;
use crate::accounts::model::Identity;
use codex::CodexEndpoints;
use serde_json::Value;

pub(crate) fn read(identity: &Identity, auth: &Value) -> Result<ProviderLimitsDto, HttpError> {
    read_at(
        identity,
        auth,
        CLAUDE_PROFILE_URL,
        CLAUDE_USAGE_URL,
        codex::CODEX,
    )
}

fn read_at(
    identity: &Identity,
    auth: &Value,
    profile: &str,
    claude_url: &str,
    codex: CodexEndpoints<'_>,
) -> Result<ProviderLimitsDto, HttpError> {
    let mut parsed = match identity.provider {
        AgentId::Claude => {
            let credential = crate::accounts::claude::ClaudeLogin::credential_in(auth)
                .ok_or(HttpError::Unauthorized)?;
            claude_read(
                &credential,
                Some(&expected_claude_identity(identity)),
                profile,
                claude_url,
                ClaudeHeaders::Saved,
            )
            .map_err(|error| match error {
                ProviderLoadError::Http(error) => error,
                ProviderLoadError::AccountMismatch => HttpError::Unauthorized,
            })?
        }
        AgentId::Codex => {
            let token = crate::accounts::codex::CodexLogin::access_token_in(auth)
                .ok_or(HttpError::Unauthorized)?;
            Parsed {
                account: None,
                reading: codex::read_wham(identity, token, codex)?,
            }
        }
        _ => return Err(HttpError::Unauthorized),
    };
    let label = parsed.account.as_ref().and_then(|a| a.label.clone());
    parsed.account = Some(LimitsAccountDto {
        id: identity.observation_key(),
        label,
        legacy_id: Some(if identity.provider == AgentId::Codex {
            identity.workspace_id.clone()
        } else {
            identity.user_id.clone()
        }),
    });
    let mut dto = finish(identity.provider, LimitsStatus::Ok, None, parsed);
    dto.current_account = false;
    if !dto.reading.has_observations() {
        return Err(HttpError::Parse(
            "Usage response contained no quota observations.".into(),
        ));
    }
    Ok(dto)
}

#[cfg(test)]
mod tests;
