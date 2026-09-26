//! The first usage reading of an isolated Codex sign-in, before its native home is removed: Codex's
//! own app-server in that home. A Claude sign-in's is a saved profile's read (`read_saved_claude`).
//! No saved vault credential is loaded or independently renewed here.
use super::*;

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
