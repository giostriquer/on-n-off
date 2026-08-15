use std::path::PathBuf;
use std::sync::Mutex;

use crate::adapter::AgentAdapter;
use crate::config_io::ConfigIo;
use crate::dto::{AdapterError, AgentId, AgentInfo, AgentTabDto};
use crate::paths::{agent_info, cursor_root};
use crate::sort::sort_tab;

pub struct CursorAdapter {
    root: Option<PathBuf>,
    io: ConfigIo,
    write: Mutex<()>,
}

impl CursorAdapter {
    pub fn new() -> Self {
        Self {
            root: cursor_root().ok(),
            io: ConfigIo::production(),
            write: Mutex::new(()),
        }
    }

    #[cfg(test)]
    pub fn at(root: PathBuf) -> Self {
        let io = ConfigIo::at(root.join("_backups"));
        Self {
            root: Some(root),
            io,
            write: Mutex::new(()),
        }
    }
}

impl AgentAdapter for CursorAdapter {
    fn info(&self) -> AgentInfo {
        let mut info = agent_info(AgentId::Cursor, "cursor-agent");
        info.install_git = false;
        info.install_folder = false;
        info.plugin_toggle = false;
        info
    }

    fn list_tab(&self) -> Result<AgentTabDto, AdapterError> {
        let mut tab = AgentTabDto {
            plugins: Vec::new(),
            user_skills: Vec::new(),
            mcp_servers: Vec::new(),
        };
        sort_tab(&mut tab);
        let _ = &self.root;
        Ok(tab)
    }

    fn set_mcp_enabled(&self, mcp_id: &str, enabled: bool) -> Result<AgentTabDto, AdapterError> {
        let _ = (mcp_id, enabled, &self.io, &self.write);
        Err(AdapterError::message("mcp toggle is not implemented yet"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn write_skill(dir: &Path, name: &str, description: &str) {
        fs::create_dir_all(dir.join(name)).unwrap();
        fs::write(
            dir.join(name).join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n"),
        )
        .unwrap();
    }

    fn write_plugin(
        dir: &Path,
        folder: &str,
        name: &str,
        skill: Option<&str>,
        cursor_manifest: bool,
    ) {
        let plugin = dir.join(folder);
        fs::create_dir_all(&plugin).unwrap();
        let manifest = if cursor_manifest {
            let nested = plugin.join(".cursor-plugin");
            fs::create_dir_all(&nested).unwrap();
            nested.join("plugin.json")
        } else {
            plugin.join("plugin.json")
        };
        fs::write(
            &manifest,
            format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
        )
        .unwrap();
        if let Some(skill_name) = skill {
            write_skill(&plugin.join("skills"), skill_name, "From plugin");
        }
    }

    #[test]
    fn empty_home_lists_nothing() {
        let root = crate::paths::scratch_dir("on-n-off-cursor-empty");
        fs::create_dir_all(&root).unwrap();
        let tab = CursorAdapter::at(root.clone()).list_tab().expect("list");
        assert!(tab.plugins.is_empty());
        assert!(tab.user_skills.is_empty());
        assert!(tab.mcp_servers.is_empty());
        let info = CursorAdapter::at(root.clone()).info();
        assert_eq!(info.id, AgentId::Cursor);
        assert_eq!(info.display_name, "Cursor");
        assert!(!info.plugin_toggle);
        assert!(!info.install_git);
        assert!(!info.install_folder);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lists_local_and_cache_plugins_user_skills_and_mcp() {
        let root = crate::paths::scratch_dir("on-n-off-cursor-list");
        write_plugin(
            &root.join("plugins").join("local"),
            "reviewer",
            "reviewer",
            Some("pr-review"),
            true,
        );
        write_plugin(
            &root.join("plugins").join("cache").join("workshop"),
            "workbench",
            "workbench",
            None,
            false,
        );
        write_skill(&root.join("skills"), "notes", "User skill");
        write_skill(&root.join("skills-cursor"), "builtin", "Managed builtin");
        fs::write(
            root.join("mcp.json"),
            r#"{"mcpServers":{"docs":{"url":"https://docs.example/mcp"},"github":{"command":"npx","args":["-y","gh"],"disabled":true}}}"#,
        )
        .unwrap();

        let tab = CursorAdapter::at(root.clone()).list_tab().expect("list");
        let local = tab
            .plugins
            .iter()
            .find(|plugin| plugin.id == "reviewer@local")
            .expect("local plugin");
        assert_eq!(local.source, "local");
        assert!(local.enabled);
        assert!(!local.togglable);
        assert_eq!(local.skills.len(), 1);
        assert_eq!(local.skills[0].name, "pr-review");
        assert!(!local.skills[0].togglable);
        let cached = tab
            .plugins
            .iter()
            .find(|plugin| plugin.id == "workbench@workshop")
            .expect("cached plugin");
        assert_eq!(cached.source, "workshop");
        assert!(!cached.togglable);
        assert_eq!(tab.user_skills.len(), 1);
        assert_eq!(tab.user_skills[0].name, "notes");
        assert!(!tab.user_skills[0].togglable);
        assert!(!tab.user_skills.iter().any(|skill| skill.name == "builtin"));
        assert_eq!(tab.mcp_servers.len(), 2);
        let docs = tab
            .mcp_servers
            .iter()
            .find(|server| server.id == "docs")
            .unwrap();
        assert!(docs.enabled);
        assert!(docs.togglable);
        let github = tab
            .mcp_servers
            .iter()
            .find(|server| server.id == "github")
            .unwrap();
        assert!(!github.enabled);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_toggle_patches_disabled_and_backs_up() {
        let root = crate::paths::scratch_dir("on-n-off-cursor-mcp-toggle");
        fs::write(
            root.join("mcp.json"),
            r#"{"mcpServers":{"github":{"command":"npx","args":["-y","gh"]}}}"#,
        )
        .unwrap();
        let adapter = CursorAdapter::at(root.clone());
        let tab = adapter.set_mcp_enabled("github", false).expect("toggle");
        assert!(!tab.mcp_servers[0].enabled);
        let text = fs::read_to_string(root.join("mcp.json")).unwrap();
        assert!(text.contains("\"disabled\": true") || text.contains("\"disabled\":true"));
        let backups = root.join("_backups").join("cursor");
        let count = fs::read_dir(&backups)
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert!(count >= 1, "expected a cursor mcp.json backup");
        let err = adapter
            .set_mcp_enabled("missing", false)
            .expect_err("missing");
        assert!(
            err.message.contains("mcp server not found") || err.message.contains("not togglable")
        );
        let _ = fs::remove_dir_all(root);
    }
}
