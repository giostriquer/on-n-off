#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallerKind {
    Development,
    Nsis,
    Dmg,
}

impl InstallerKind {
    pub const fn name(self) -> Option<&'static str> {
        match self {
            Self::Development => None,
            Self::Nsis => Some("nsis"),
            Self::Dmg => Some("dmg"),
        }
    }

    /// Updater platform key; must match the `platforms` entry the release feed publishes.
    pub const fn target(self) -> Option<&'static str> {
        match self {
            Self::Development => None,
            Self::Nsis => Some("windows-x86_64-nsis"),
            Self::Dmg => Some("darwin-aarch64"),
        }
    }
}

pub fn parse_installer_kind(value: Option<&str>) -> Result<InstallerKind, String> {
    match value {
        None => Ok(InstallerKind::Development),
        Some("nsis") => Ok(InstallerKind::Nsis),
        Some("dmg") => Ok(InstallerKind::Dmg),
        Some(other) => Err(format!(
            "ON_N_OFF_INSTALLER_KIND must be 'nsis' or 'dmg'; got '{other}'"
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
        assert_eq!(parse_installer_kind(Some("dmg")), Ok(InstallerKind::Dmg));
    }

    #[test]
    fn rejects_unknown_installer_kind() {
        let error = parse_installer_kind(Some("portable")).unwrap_err();

        assert_eq!(
            error,
            "ON_N_OFF_INSTALLER_KIND must be 'nsis' or 'dmg'; got 'portable'"
        );
    }

    #[test]
    fn assigns_distinct_updater_targets_to_each_installer_format() {
        assert_eq!(InstallerKind::Development.name(), None);
        assert_eq!(InstallerKind::Development.target(), None);
        assert_eq!(InstallerKind::Nsis.name(), Some("nsis"));
        assert_eq!(InstallerKind::Nsis.target(), Some("windows-x86_64-nsis"));
        assert_eq!(InstallerKind::Dmg.name(), Some("dmg"));
        assert_eq!(InstallerKind::Dmg.target(), Some("darwin-aarch64"));
    }

    #[test]
    fn base_config_keeps_the_updater_plugin_runnable_in_development_builds() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &config["plugins"]["updater"];

        assert!(
            updater.is_object(),
            "plugins.updater must not serialize as null"
        );
        assert!(
            updater["pubkey"]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "plugins.updater.pubkey must be configured"
        );
        assert!(
            updater["endpoints"]
                .as_array()
                .is_some_and(|values| !values.is_empty()),
            "plugins.updater.endpoints must be configured"
        );
    }

    #[test]
    fn updater_overlay_only_enables_release_artifacts() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.updater.conf.json")).unwrap();

        assert_eq!(config["bundle"]["createUpdaterArtifacts"], true);
        assert!(
            config.get("plugins").is_none(),
            "the base config must be the single source of updater runtime settings"
        );
    }
}
