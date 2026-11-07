use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::adapter::{AgentAdapter, ItemPlaces};
use crate::dto::{AdapterError, AgentInfo, AgentTabDto, PluginDto};
use crate::install_source::{parse_install_source, InstallSource, SourceKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Capabilities {
    pub plugin_switch: Change,
    pub skill_switch: Change,
    pub mcp_switch: Change,
    pub install: Change,
    pub refused_sources: BTreeMap<SourceKind, String>,
    pub uninstall: Change,
    pub uninstall_from: Option<Vec<String>>,
    pub update: Change,
    pub reads_hooks: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        let unknown = || Change::refused("", "");
        Self {
            plugin_switch: unknown(),
            skill_switch: unknown(),
            mcp_switch: unknown(),
            install: unknown(),
            refused_sources: BTreeMap::new(),
            uninstall: unknown(),
            uninstall_from: None,
            update: unknown(),
            reads_hooks: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "by", rename_all = "camelCase")]
pub enum Change {
    Cli { how: String },
    File { how: String },
    Refused { how: String, reason: String },
}

impl Change {
    pub fn cli(how: &str) -> Self {
        Self::Cli {
            how: how.to_string(),
        }
    }

    pub fn file(how: &str) -> Self {
        Self::File {
            how: how.to_string(),
        }
    }

    pub fn refused(how: &str, reason: &str) -> Self {
        Self::Refused {
            how: how.to_string(),
            reason: reason.to_string(),
        }
    }

    pub fn not_implemented(what: &str) -> Self {
        Self::refused(
            "not implemented yet",
            &format!("{what} is not implemented yet"),
        )
    }

    pub fn is_refused(&self) -> bool {
        matches!(self, Self::Refused { .. })
    }

    pub fn ready(&self, cli_found: bool) -> bool {
        match self {
            Self::Cli { .. } => cli_found,
            Self::File { .. } => true,
            Self::Refused { .. } => false,
        }
    }

    pub fn refusal(&self) -> AdapterError {
        match self {
            Self::Refused { reason, .. } => AdapterError::message(reason.clone()),
            Self::Cli { how } | Self::File { how } => {
                AdapterError::message(format!("{how} is not implemented yet"))
            }
        }
    }
}

impl Capabilities {
    pub fn accepts(&self, kind: SourceKind) -> bool {
        !self.refused_sources.contains_key(&kind)
    }

    pub fn admit(&self, source: &str) -> Result<InstallSource, AdapterError> {
        let parsed = parse_install_source(source)?;
        match self.refused_sources.get(&parsed.kind()) {
            Some(reason) => Err(AdapterError::message(reason.clone())),
            None => Ok(parsed),
        }
    }

    pub fn restrict(&self, tab: &mut AgentTabDto) {
        let plugins = !self.plugin_switch.is_refused();
        let skills = !self.skill_switch.is_refused();
        let servers = !self.mcp_switch.is_refused();
        for plugin in &mut tab.plugins {
            plugin.togglable &= plugins;
            for skill in &mut plugin.skills {
                skill.togglable &= skills;
            }
        }
        for skill in &mut tab.user_skills {
            skill.togglable &= skills;
        }
        for server in &mut tab.mcp_servers {
            server.togglable &= servers;
        }
    }

    pub fn uninstalls(&self, plugin: &PluginDto) -> Result<(), AdapterError> {
        match &self.uninstall_from {
            Some(sources) if !sources.contains(&plugin.source) => Err(AdapterError::message(
                format!("plugin cannot be uninstalled here: {}", plugin.id),
            )),
            _ => Ok(()),
        }
    }
}

impl AgentInfo {
    pub fn of(adapter: &dyn AgentAdapter) -> Self {
        Self::describe(
            adapter,
            crate::cli_locate::provider_cli_found(adapter.id()),
            crate::paths::user_home().ok().as_deref(),
        )
    }

    pub fn describe<A: AgentAdapter + ?Sized>(
        adapter: &A,
        cli_found: bool,
        home: Option<&Path>,
    ) -> Self {
        let id = adapter.id();
        let capabilities = adapter.capabilities();
        let installs = |kind| capabilities.install.ready(cli_found) && capabilities.accepts(kind);
        Self {
            id,
            display_name: id.display_name().to_string(),
            cli_ok: cli_found,
            cli_error: (!cli_found).then(|| format!("{} CLI not found.", id.display_name())),
            install_git: installs(SourceKind::GitMarketplace),
            install_folder: installs(SourceKind::Folder),
            plugin_toggle: capabilities.plugin_switch.ready(cli_found),
            reads_hooks: capabilities.reads_hooks,
            item_roots: ItemPlaces::of(adapter, home),
            capabilities,
        }
    }
}

#[cfg(test)]
mod tests;
