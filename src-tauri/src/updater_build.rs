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

// build.rs also includes this file via `#[path = "src/updater_build.rs"]`, which changes the
// directory `mod tests;` would resolve against; pin it so both inclusion trees agree.
#[cfg(test)]
#[path = "updater_build/tests.rs"]
mod tests;
