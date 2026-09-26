//! First usage observation during isolated official sign-in, before its native home is removed.
//! No saved vault credential is loaded or independently renewed here.
use super::{credentials::ClaudeCredential, *};

/// The first usage reading of an isolated Claude sign-in as `identity`: a saved profile's read
/// (`read_saved_claude`) with its login's `credential`.
pub(crate) fn read_claude(
    identity: &Identity,
    credential: ClaudeCredential,
) -> Option<ProviderLimitsDto> {
    read_saved_claude(identity, Some(credential), Utc::now().timestamp_millis()).ok()
}

/// The first usage reading of an isolated Codex sign-in as `identity`, read by Codex's own
/// app-server in `isolated_home`, which needs nothing from the login itself.
pub(crate) fn read_codex(isolated_home: &Path, identity: &Identity) -> Option<ProviderLimitsDto> {
    accepted(identity, codex_limits(isolated_home, false))
}

/// `dto`, when it is an answer about `identity` that observed something, as a card that is not the
/// signed-in account's.
fn accepted(identity: &Identity, mut dto: ProviderLimitsDto) -> Option<ProviderLimitsDto> {
    if dto.status != LimitsStatus::Ok
        || dto.account.as_ref()?.id != identity.observation_key()
        || !dto.reading.has_observations()
    {
        return None;
    }
    dto.current_account = false;
    Some(dto)
}

#[cfg(test)]
mod tests;
