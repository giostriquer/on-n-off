#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallerKind {
    Development,
    Nsis,
    Msi,
}

impl InstallerKind {
    pub const fn name(self) -> Option<&'static str> {
        match self {
            Self::Development => None,
            Self::Nsis => Some("nsis"),
            Self::Msi => Some("msi"),
        }
    }

    pub const fn target(self) -> Option<&'static str> {
        match self {
            Self::Development => None,
            Self::Nsis => Some("windows-x86_64-nsis"),
            Self::Msi => Some("windows-x86_64-msi"),
        }
    }
}

pub fn parse_installer_kind(value: Option<&str>) -> Result<InstallerKind, String> {
    match value {
        None => Ok(InstallerKind::Development),
        Some("nsis") => Ok(InstallerKind::Nsis),
        Some("msi") => Ok(InstallerKind::Msi),
        Some(other) => Err(format!(
            "ON_N_OFF_INSTALLER_KIND must be 'nsis' or 'msi'; got '{other}'"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_installer_kinds_and_development_fallback() {
        assert_eq!(parse_installer_kind(None), Ok(InstallerKind::Development));
        assert_eq!(parse_installer_kind(Some("nsis")), Ok(InstallerKind::Nsis));
        assert_eq!(parse_installer_kind(Some("msi")), Ok(InstallerKind::Msi));
    }

    #[test]
    fn rejects_unknown_installer_kind() {
        let error = parse_installer_kind(Some("portable")).unwrap_err();

        assert_eq!(
            error,
            "ON_N_OFF_INSTALLER_KIND must be 'nsis' or 'msi'; got 'portable'"
        );
    }

    #[test]
    fn assigns_distinct_updater_targets_to_each_installer_format() {
        assert_eq!(InstallerKind::Development.name(), None);
        assert_eq!(InstallerKind::Development.target(), None);
        assert_eq!(InstallerKind::Nsis.name(), Some("nsis"));
        assert_eq!(InstallerKind::Nsis.target(), Some("windows-x86_64-nsis"));
        assert_eq!(InstallerKind::Msi.name(), Some("msi"));
        assert_eq!(InstallerKind::Msi.target(), Some("windows-x86_64-msi"));
    }
}
