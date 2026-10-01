use super::*;

pub(crate) fn read_codex(isolated_home: &Path, identity: &Identity) -> Option<ProviderLimitsDto> {
    accepted(identity, codex::codex_limits(isolated_home, false))
}

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
