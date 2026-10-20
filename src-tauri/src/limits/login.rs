use super::*;

pub(crate) fn read_codex(isolated_home: &Path, identity: &Identity) -> Option<ProviderLimitsDto> {
    outcome::first_usage(identity, codex::codex_read(isolated_home, false).answer)
}

#[cfg(test)]
mod tests;
