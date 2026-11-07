use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::capabilities::Capabilities;
use crate::change::{Checked, Step};
use crate::drift::Marketplace;
use crate::dto::{AdapterError, AgentId, AgentTabDto, ItemScope, ProjectDto};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRoots {
    pub skills: std::path::PathBuf,
    pub agents: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ItemPlaces {
    pub global: Option<ItemPlace>,
    pub project: Option<ItemPlace>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ItemPlace {
    pub skills: String,
    pub agents: Option<String>,
}

impl ItemPlaces {
    pub fn of<A: AgentAdapter + ?Sized>(adapter: &A, home: Option<&Path>) -> Self {
        let place = |scope: &ItemScope| {
            adapter
                .item_roots(scope)
                .ok()
                .map(|roots| ItemPlace::shown(&roots, home))
        };
        Self {
            global: place(&ItemScope::Global),
            project: place(&ItemScope::Project {
                project_path: String::new(),
            }),
        }
    }
}

impl ItemPlace {
    fn shown(roots: &ItemRoots, home: Option<&Path>) -> Self {
        Self {
            skills: shown(&roots.skills, home),
            agents: roots.agents.as_deref().map(|agents| shown(agents, home)),
        }
    }
}

fn shown(path: &Path, home: Option<&Path>) -> String {
    let words = |path: &Path| {
        path.components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    if let Some(rest) = home.and_then(|home| path.strip_prefix(home).ok()) {
        return std::iter::once("~".to_string())
            .chain(words(rest))
            .collect::<Vec<_>>()
            .join("/");
    }
    if path.is_relative() {
        return words(path).join("/");
    }
    path.display().to_string()
}

#[cfg(test)]
pub fn scratch_adapter(id: AgentId, home: &Path) -> Box<dyn AgentAdapter> {
    match id {
        AgentId::Claude => Box::new(crate::claude::ClaudeAdapter::at(home.join(".claude"))),
        AgentId::Codex => Box::new(crate::codex::CodexAdapter::at(
            home.join(".codex"),
            home.join(".agents").join("skills"),
        )),
        AgentId::Antigravity => Box::new(crate::antigravity::AntigravityAdapter::at(
            home.join(".gemini"),
        )),
        AgentId::Cursor => Box::new(crate::cursor::CursorAdapter::at(home.join(".cursor"))),
        AgentId::Muse => Box::new(crate::muse::MuseAdapter::at(home.to_path_buf())),
    }
}

pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> AgentId;
    fn capabilities(&self) -> Capabilities;
    fn supports_accounts(&self) -> bool {
        false
    }
    fn list_tab(&self) -> Result<AgentTabDto, AdapterError>;
    fn marketplaces(&self) -> Vec<Marketplace> {
        Vec::new()
    }
    fn list_projects(&self) -> Vec<ProjectDto> {
        Vec::new()
    }
    fn list_scope(&self, project: Option<&str>) -> Result<AgentTabDto, AdapterError> {
        let mut tab = self.list_tab()?;
        if let Some(path) = project.map(str::trim).filter(|value| !value.is_empty()) {
            crate::project::overlay_project(&mut tab, Path::new(path), self.id());
        }
        Ok(tab)
    }

    fn step(&self, change: &Checked) -> Result<Option<Step<'_>>, AdapterError> {
        let _ = change;
        Ok(None)
    }

    fn item_roots(&self, scope: &ItemScope) -> Result<ItemRoots, AdapterError> {
        let _ = scope;
        Err(AdapterError::message(
            "local skill install is not supported for this agent",
        ))
    }
}

#[cfg(test)]
mod tests;
