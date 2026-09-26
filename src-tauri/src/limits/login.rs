//! First usage observation during isolated official sign-in, before its native home is removed.
//! No saved vault credential is loaded or independently renewed here.
use super::{credentials::ClaudeCredential, *};

/// The first usage reading of an isolated Claude sign-in as `identity`: a saved profile's read
/// (`read_saved_claude`) with its login's `credential`.
pub(crate) fn read_claude(
    identity: &Identity,
    credential: ClaudeCredential,
) -> Option<ProviderLimitsDto> {
    read_saved_claude(identity, Some(credential)).ok()
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

/// Called only after the account registry accepted this sign-in's identity and operation epoch.
pub(crate) fn remember(home: &Path, usage: &ProviderLimitsDto) -> Result<(), String> {
    SnapshotStore::for_home(home).save(usage)
}

/// What [`remember`] left on disk for `provider`, as the next read loads it.
#[cfg(test)]
pub(crate) fn remembered(home: &Path, provider: AgentId) -> Vec<ProviderLimitsDto> {
    SnapshotStore::for_home(home).load(provider)
}

#[cfg(test)]
mod tests;
