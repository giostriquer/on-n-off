use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::Value as JsonValue;
use toml_edit::DocumentMut;

use crate::adapter::AgentAdapter;
use crate::capabilities::{Capabilities, Change};
use crate::cli::{run_npx_skills, AgentCli, DEFAULT_TIMEOUT, INSTALL_TIMEOUT};
use crate::config_io::{Backups, ConfigIo, Edit};
use crate::dto::{AdapterError, AgentId, AgentTabDto, McpServerDto, PluginDto, SkillDto};
use crate::install_source::InstallSource;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    SwitchPlugin { id: String, on: bool },
    SwitchSkill { id: String, on: bool },
    SwitchMcp { id: String, on: bool },
    Install { source: String },
    Uninstall { id: String },
    Update { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    Plugin { plugin: PluginDto, on: bool },
    Skill { skill: SkillDto, on: bool },
    Mcp { server: McpServerDto, on: bool },
    Install(InstallSource),
    Uninstall(PluginDto),
    Update(PluginDto),
}

pub struct Step<'a> {
    touches: Vec<PathBuf>,
    act: Act<'a>,
}

enum Act<'a> {
    Cli {
        cli: &'a AgentCli,
        runs: Vec<Vec<String>>,
    },
    Npx {
        source: InstallSource,
        agent: &'static str,
    },
    Patch {
        path: PathBuf,
        edit: Edit<'a>,
    },
}

#[cfg(test)]
type Fake = Box<dyn Fn(&Step<'_>, Duration) -> Result<(), AdapterError> + Send + Sync>;

pub struct Changes {
    io: ConfigIo,
    turns: [Mutex<()>; AgentId::ALL.len()],
    #[cfg(test)]
    fake: Option<Fake>,
}

pub fn switch(on: bool) -> &'static str {
    if on {
        "enable"
    } else {
        "disable"
    }
}

impl<'a> Step<'a> {
    pub fn cli(cli: &'a AgentCli, touches: Vec<PathBuf>, runs: Vec<Vec<String>>) -> Self {
        Self {
            touches,
            act: Act::Cli { cli, runs },
        }
    }

    pub fn npx(source: InstallSource, agent: &'static str) -> Self {
        Self {
            touches: Vec::new(),
            act: Act::Npx { source, agent },
        }
    }

    pub fn json(
        path: PathBuf,
        edit: impl FnOnce(&mut JsonValue) -> Result<(), AdapterError> + 'a,
    ) -> Self {
        Self::patch(path, Edit::json(edit))
    }

    pub fn toml(
        path: PathBuf,
        edit: impl FnOnce(&mut DocumentMut) -> Result<(), AdapterError> + 'a,
    ) -> Self {
        Self::patch(path, Edit::toml(edit))
    }

    fn patch(path: PathBuf, edit: Edit<'a>) -> Self {
        Self {
            touches: vec![path.clone()],
            act: Act::Patch { path, edit },
        }
    }

    fn run(self, io: &ConfigIo, backups: &Backups, deadline: Duration) -> Result<(), AdapterError> {
        match self.act {
            Act::Cli { cli, runs } => runs
                .iter()
                .try_for_each(|argv| cli.run_args_timed(argv, deadline).map(drop)),
            Act::Npx { source, agent } => run_npx_skills(&source, agent).map(drop),
            Act::Patch { path, edit } => io.patch(&path, backups, edit),
        }
    }
}

#[cfg(test)]
impl Step<'_> {
    pub fn touches(&self) -> &[PathBuf] {
        &self.touches
    }

    pub fn argv(&self) -> &[Vec<String>] {
        match &self.act {
            Act::Cli { runs, .. } => runs,
            Act::Npx { .. } | Act::Patch { .. } => &[],
        }
    }

    pub fn npx_agent(&self) -> Option<&str> {
        match &self.act {
            Act::Npx { agent, .. } => Some(agent),
            Act::Cli { .. } | Act::Patch { .. } => None,
        }
    }
}

#[cfg(test)]
impl Request {
    pub fn plugin(id: &str, on: bool) -> Self {
        Self::SwitchPlugin { id: id.into(), on }
    }

    pub fn skill(id: &str, on: bool) -> Self {
        Self::SwitchSkill { id: id.into(), on }
    }

    pub fn mcp(id: &str, on: bool) -> Self {
        Self::SwitchMcp { id: id.into(), on }
    }

    pub fn install(source: &str) -> Self {
        Self::Install {
            source: source.into(),
        }
    }

    pub fn uninstall(id: &str) -> Self {
        Self::Uninstall { id: id.into() }
    }

    pub fn update(id: &str) -> Self {
        Self::Update { id: id.into() }
    }
}

#[cfg(test)]
pub fn row(id: &str) -> PluginDto {
    let (name, source) = crate::paths::plugin_id_parts(id);
    PluginDto {
        id: id.into(),
        name,
        source,
        version: String::new(),
        upstream: String::new(),
        out_of_sync: false,
        enabled: true,
        togglable: true,
        skills: Vec::new(),
    }
}

#[cfg(test)]
pub fn argv_of(adapter: &dyn AgentAdapter, change: &Checked) -> Vec<Vec<String>> {
    adapter
        .step(change)
        .unwrap()
        .expect("a step")
        .argv()
        .to_vec()
}

impl Request {
    fn declared<'c>(&self, capabilities: &'c Capabilities) -> &'c Change {
        match self {
            Self::SwitchPlugin { .. } => &capabilities.plugin_switch,
            Self::SwitchSkill { .. } => &capabilities.skill_switch,
            Self::SwitchMcp { .. } => &capabilities.mcp_switch,
            Self::Install { .. } => &capabilities.install,
            Self::Uninstall { .. } => &capabilities.uninstall,
            Self::Update { .. } => &capabilities.update,
        }
    }

    fn deadline(&self) -> Duration {
        match self {
            Self::Install { .. } | Self::Update { .. } => INSTALL_TIMEOUT,
            Self::SwitchPlugin { .. }
            | Self::SwitchSkill { .. }
            | Self::SwitchMcp { .. }
            | Self::Uninstall { .. } => DEFAULT_TIMEOUT,
        }
    }

    fn check(
        &self,
        declared: &Capabilities,
        read: impl FnOnce() -> Result<AgentTabDto, AdapterError>,
    ) -> Result<Checked, AdapterError> {
        Ok(match self {
            Self::Install { source } => Checked::Install(declared.admit(source)?),
            Self::SwitchPlugin { id, on } => Checked::Plugin {
                plugin: switchable_plugin(&read()?, id)?.clone(),
                on: *on,
            },
            Self::SwitchSkill { id, on } => Checked::Skill {
                skill: switchable_skill(&read()?, id)?.clone(),
                on: *on,
            },
            Self::SwitchMcp { id, on } => Checked::Mcp {
                server: switchable_server(&read()?, id)?.clone(),
                on: *on,
            },
            Self::Uninstall { id } => {
                let shown = read()?;
                let plugin = plugin(&shown, id)?;
                declared.uninstalls(plugin)?;
                Checked::Uninstall(plugin.clone())
            }
            Self::Update { id } => Checked::Update(plugin(&read()?, id)?.clone()),
        })
    }
}

fn plugin<'v>(shown: &'v AgentTabDto, id: &str) -> Result<&'v PluginDto, AdapterError> {
    shown
        .plugins
        .iter()
        .find(|plugin| plugin.id == id)
        .ok_or_else(|| AdapterError::message(format!("plugin not found: {id}")))
}

fn switchable_plugin<'v>(shown: &'v AgentTabDto, id: &str) -> Result<&'v PluginDto, AdapterError> {
    let plugin = plugin(shown, id)?;
    if !plugin.togglable {
        return Err(AdapterError::message(format!(
            "plugin is not togglable: {id}"
        )));
    }
    Ok(plugin)
}

fn switchable_skill<'v>(shown: &'v AgentTabDto, id: &str) -> Result<&'v SkillDto, AdapterError> {
    let skill = shown
        .plugins
        .iter()
        .flat_map(|plugin| plugin.skills.iter())
        .chain(shown.user_skills.iter())
        .find(|skill| skill.id == id)
        .ok_or_else(|| AdapterError::message(format!("skill not found: {id}")))?;
    if !skill.togglable {
        return Err(AdapterError::message(format!(
            "skill is not togglable: {id}"
        )));
    }
    Ok(skill)
}

fn switchable_server<'v>(
    shown: &'v AgentTabDto,
    id: &str,
) -> Result<&'v McpServerDto, AdapterError> {
    let server = shown
        .mcp_servers
        .iter()
        .find(|server| server.id == id)
        .ok_or_else(|| AdapterError::message(format!("mcp server not found: {id}")))?;
    if !server.togglable {
        return Err(AdapterError::message(format!(
            "mcp server is not togglable: {id}"
        )));
    }
    Ok(server)
}

impl Changes {
    pub fn production() -> Self {
        Self::with(ConfigIo::production())
    }

    #[cfg(test)]
    pub fn at(backups: impl Into<PathBuf>) -> Self {
        Self::with(ConfigIo::at(backups))
    }

    #[cfg(test)]
    pub fn beside(root: &std::path::Path) -> Self {
        Self::at(root.join("_backups"))
    }

    #[cfg(test)]
    pub fn global(
        &self,
        adapter: &dyn AgentAdapter,
        request: Request,
    ) -> Result<AgentTabDto, AdapterError> {
        self.make(adapter, request, None)
    }

    #[cfg(test)]
    pub fn faking(
        backups: impl Into<PathBuf>,
        fake: impl Fn(&Step<'_>, Duration) -> Result<(), AdapterError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            fake: Some(Box::new(fake)),
            ..Self::at(backups)
        }
    }

    fn with(io: ConfigIo) -> Self {
        Self {
            io,
            turns: std::array::from_fn(|_| Mutex::new(())),
            #[cfg(test)]
            fake: None,
        }
    }

    pub fn make(
        &self,
        adapter: &dyn AgentAdapter,
        request: Request,
        view: Option<&str>,
    ) -> Result<AgentTabDto, AdapterError> {
        let id = adapter.id();
        let _turn = self.turn(id);
        let declared = adapter.capabilities();
        let change = request.declared(&declared);
        if change.is_refused() {
            return Err(change.refusal());
        }
        let checked = request.check(&declared, || adapter.list_scope(view))?;
        let Some(step) = adapter.step(&checked)? else {
            return Err(change.refusal());
        };
        let backups = self.io.back_up(id, &step.touches)?;
        self.run(step, &backups, request.deadline())?;
        crate::drift::read(adapter, view, crate::drift::Reach::Local)
    }

    fn run(
        &self,
        step: Step<'_>,
        backups: &Backups,
        deadline: Duration,
    ) -> Result<(), AdapterError> {
        #[cfg(test)]
        if let Some(fake) = &self.fake {
            return fake(&step, deadline);
        }
        step.run(&self.io, backups, deadline)
    }

    fn turn(&self, id: AgentId) -> MutexGuard<'_, ()> {
        let index = match id {
            AgentId::Claude => 0,
            AgentId::Codex => 1,
            AgentId::Antigravity => 2,
            AgentId::Cursor => 3,
            AgentId::Muse => 4,
        };
        self.turns[index]
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests;
