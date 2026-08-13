use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use serde_json::Value as JsonValue;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

use crate::backup::BackupStore;
use crate::dto::{AdapterError, AgentId};
use crate::paths::normalize_skill_path;

pub struct ConfigIo {
    backups: BackupStore,
}

impl ConfigIo {
    pub fn new() -> Result<Self, AdapterError> {
        Ok(Self {
            backups: BackupStore::new()?,
        })
    }

    pub fn at(backup_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            backups: BackupStore::at(backup_root.into()),
        }
    }

    pub fn production() -> Self {
        Self::new().unwrap_or_else(|_| Self::at(std::env::temp_dir().join("on-n-off-backups")))
    }

    #[cfg(test)]
    fn force_invalid_commit(&self, agent: AgentId, path: &Path) -> Result<(), AdapterError> {
        let original = read_or_empty(path)?;
        self.commit(agent, path, &original, "{not json", |text| {
            serde_json::from_str::<JsonValue>(text).map(|_| ())
        })
    }

    pub fn patch_json_skill_override(
        &self,
        agent: AgentId,
        path: &Path,
        skill_name: &str,
        enabled: bool,
    ) -> Result<(), AdapterError> {
        self.patch_json(agent, path, |root| {
            let object = root.as_object_mut().ok_or_else(|| {
                AdapterError::message("settings.json root must be an object")
            })?;
            let overrides = object
                .entry("skillOverrides")
                .or_insert_with(|| JsonValue::Object(serde_json::Map::new()));
            let overrides = overrides.as_object_mut().ok_or_else(|| {
                AdapterError::message("skillOverrides must be an object")
            })?;
            overrides.insert(
                skill_name.to_string(),
                JsonValue::String(if enabled { "on" } else { "off" }.to_string()),
            );
            Ok(())
        })
    }

    pub fn backup_file(&self, agent: AgentId, path: &Path) -> Result<Option<std::path::PathBuf>, AdapterError> {
        self.backups.backup(agent, path)
    }

    pub fn patch_json_mcp_enabled(
        &self,
        agent: AgentId,
        path: &Path,
        mcp_id: &str,
        enabled: bool,
    ) -> Result<(), AdapterError> {
        self.patch_json(agent, path, |root| {
            let object = root.as_object_mut().ok_or_else(|| {
                AdapterError::message("claude.json root must be an object")
            })?;
            let servers = object
                .get_mut("mcpServers")
                .and_then(JsonValue::as_object_mut)
                .ok_or_else(|| AdapterError::message("mcpServers missing"))?;
            let entry = servers
                .get_mut(mcp_id)
                .and_then(JsonValue::as_object_mut)
                .ok_or_else(|| AdapterError::message(format!("mcp server not found: {mcp_id}")))?;
            entry.insert("disabled".to_string(), JsonValue::Bool(!enabled));
            let list = object
                .entry("disabledMcpServers")
                .or_insert_with(|| JsonValue::Array(Vec::new()));
            let list = list.as_array_mut().ok_or_else(|| {
                AdapterError::message("disabledMcpServers must be an array")
            })?;
            list.retain(|item| item.as_str() != Some(mcp_id));
            if !enabled {
                list.push(JsonValue::String(mcp_id.to_string()));
            }
            Ok(())
        })
    }

    pub fn patch_antigravity_mcp_enabled(
        &self,
        agent: AgentId,
        path: &Path,
        mcp_id: &str,
        enabled: bool,
    ) -> Result<(), AdapterError> {
        self.patch_json(agent, path, |root| {
            let object = root.as_object_mut().ok_or_else(|| {
                AdapterError::message("mcp_config.json root must be an object")
            })?;
            let servers = object
                .get_mut("mcpServers")
                .and_then(JsonValue::as_object_mut)
                .ok_or_else(|| AdapterError::message("mcpServers missing"))?;
            let entry = servers
                .get_mut(mcp_id)
                .and_then(JsonValue::as_object_mut)
                .ok_or_else(|| AdapterError::message(format!("mcp server not found: {mcp_id}")))?;
            entry.insert("disabled".to_string(), JsonValue::Bool(!enabled));
            Ok(())
        })
    }

    pub fn patch_toml_plugin_enabled(
        &self,
        agent: AgentId,
        path: &Path,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<(), AdapterError> {
        self.patch_toml(agent, path, |doc| {
            let plugins = doc
                .get_mut("plugins")
                .and_then(Item::as_table_mut)
                .ok_or_else(|| AdapterError::message("plugins table missing"))?;
            let entry = plugins
                .get_mut(plugin_id)
                .and_then(Item::as_table_mut)
                .ok_or_else(|| AdapterError::message(format!("plugin not in config: {plugin_id}")))?;
            entry["enabled"] = value(enabled);
            Ok(())
        })
    }

    pub fn patch_toml_mcp_enabled(
        &self,
        agent: AgentId,
        path: &Path,
        mcp_id: &str,
        enabled: bool,
    ) -> Result<(), AdapterError> {
        self.patch_toml(agent, path, |doc| {
            let servers = doc
                .get_mut("mcp_servers")
                .and_then(Item::as_table_mut)
                .ok_or_else(|| AdapterError::message("mcp_servers table missing"))?;
            let entry = servers
                .get_mut(mcp_id)
                .and_then(Item::as_table_mut)
                .ok_or_else(|| AdapterError::message(format!("mcp server not found: {mcp_id}")))?;
            entry["enabled"] = value(enabled);
            Ok(())
        })
    }

    pub fn patch_toml_skill_enabled(
        &self,
        agent: AgentId,
        path: &Path,
        skill_path: &str,
        enabled: bool,
    ) -> Result<(), AdapterError> {
        self.patch_toml(agent, path, |doc| {
            let tables = ensure_skills_config(doc)?;
            let key = normalize_skill_path(skill_path);
            for table in tables.iter_mut() {
                let Some(existing) = table.get("path").and_then(Item::as_str) else {
                    continue;
                };
                if normalize_skill_path(existing) == key {
                    table["enabled"] = value(enabled);
                    return Ok(());
                }
            }
            let mut table = Table::new();
            table["path"] = value(skill_path);
            table["enabled"] = value(enabled);
            tables.push(table);
            Ok(())
        })
    }

    fn patch_json(
        &self,
        agent: AgentId,
        path: &Path,
        patch: impl FnOnce(&mut JsonValue) -> Result<(), AdapterError>,
    ) -> Result<(), AdapterError> {
        let original = read_or_empty(path)?;
        let mut value: JsonValue = if original.trim().is_empty() {
            JsonValue::Object(serde_json::Map::new())
        } else {
            serde_json::from_str(&original).map_err(|error| {
                AdapterError::parse(error.to_string(), Some(path.display().to_string()))
            })?
        };
        patch(&mut value)?;
        let rendered = format!("{}\n", serde_json::to_string_pretty(&value).map_err(|error| {
            AdapterError::write(error.to_string(), Some(path.display().to_string()))
        })?);
        self.commit(agent, path, &original, &rendered, |text| {
            serde_json::from_str::<JsonValue>(text).map(|_| ())
        })
    }

    fn patch_toml(
        &self,
        agent: AgentId,
        path: &Path,
        patch: impl FnOnce(&mut DocumentMut) -> Result<(), AdapterError>,
    ) -> Result<(), AdapterError> {
        let original = read_or_empty(path)?;
        let mut doc: DocumentMut = if original.trim().is_empty() {
            DocumentMut::new()
        } else {
            original.parse().map_err(|error: toml_edit::TomlError| {
                AdapterError::parse(error.to_string(), Some(path.display().to_string()))
            })?
        };
        patch(&mut doc)?;
        let rendered = doc.to_string();
        self.commit(agent, path, &original, &rendered, |text| {
            text.parse::<DocumentMut>().map(|_| ())
        })
    }

    fn commit<E>(
        &self,
        agent: AgentId,
        path: &Path,
        original: &str,
        rendered: &str,
        parse: impl Fn(&str) -> Result<(), E>,
    ) -> Result<(), AdapterError> {
        let backup = if path.exists() {
            self.backups.backup(agent, path)?
        } else {
            None
        };
        if let Err(error) = atomic_replace(path, rendered) {
            rollback(&self.backups, backup.as_deref(), path, original);
            return Err(error);
        }
        if parse(&fs::read_to_string(path).unwrap_or_default()).is_err() {
            rollback(&self.backups, backup.as_deref(), path, original);
            return Err(AdapterError::write(
                "Write failed. Restored the previous file.",
                Some(path.display().to_string()),
            ));
        }
        Ok(())
    }
}

fn ensure_skills_config(doc: &mut DocumentMut) -> Result<&mut ArrayOfTables, AdapterError> {
    if doc.get("skills").and_then(Item::as_table).is_none() {
        let mut skills = Table::new();
        skills.set_implicit(true);
        skills["config"] = Item::ArrayOfTables(ArrayOfTables::new());
        doc.insert("skills", Item::Table(skills));
    } else if !doc
        .get("skills")
        .and_then(|skills| skills.get("config"))
        .is_some_and(Item::is_array_of_tables)
    {
        let skills = doc
            .get_mut("skills")
            .and_then(Item::as_table_mut)
            .ok_or_else(|| AdapterError::message("skills must be a table"))?;
        skills["config"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    doc.get_mut("skills")
        .and_then(|skills| skills.get_mut("config"))
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| AdapterError::message("skills.config must be an array of tables"))
}

fn read_or_empty(path: &Path) -> Result<String, AdapterError> {
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(path).map_err(|error| {
        AdapterError::parse(error.to_string(), Some(path.display().to_string()))
    })
}

fn rollback(store: &BackupStore, backup: Option<&Path>, dest: &Path, original: &str) {
    if let Some(backup) = backup {
        let _ = store.restore(backup, dest);
        return;
    }
    if original.is_empty() {
        let _ = fs::remove_file(dest);
    } else {
        let _ = fs::write(dest, original);
    }
}

fn temp_sibling(path: &Path) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp-on-n-off");
    std::path::PathBuf::from(name)
}

fn atomic_replace(path: &Path, contents: &str) -> Result<(), AdapterError> {
    let tmp = temp_sibling(path);
    {
        let mut file = File::create(&tmp).map_err(|error| {
            AdapterError::write(error.to_string(), Some(tmp.display().to_string()))
        })?;
        file.write_all(contents.as_bytes()).map_err(|error| {
            AdapterError::write(error.to_string(), Some(tmp.display().to_string()))
        })?;
        file.sync_all().map_err(|error| {
            AdapterError::write(error.to_string(), Some(tmp.display().to_string()))
        })?;
    }
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path).map_err(|error| {
            AdapterError::write(error.to_string(), Some(path.display().to_string()))
        })?;
    }
    let result = fs::rename(&tmp, path).map_err(|error| {
        AdapterError::write(error.to_string(), Some(path.display().to_string()))
    });
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::ErrorKind;
    use serde_json::json;

    #[test]
    fn json_patch_only_changes_skill_overrides_and_keeps_unrelated_keys() {
        let root = crate::paths::scratch_dir("on-n-off-json");
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{
  "model": "opus",
  "enabledPlugins": { "workbench@workshop": true },
  "skillOverrides": { "legacy": "name-only" }
}"#,
        )
        .unwrap();
        let io = ConfigIo::at(root.join("backups"));
        io.patch_json_skill_override(AgentId::Claude, &path, "statusline", false)
            .unwrap();
        let value: JsonValue = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["model"], "opus");
        assert_eq!(value["enabledPlugins"]["workbench@workshop"], true);
        assert_eq!(value["skillOverrides"]["legacy"], "name-only");
        assert_eq!(value["skillOverrides"]["statusline"], "off");
        assert!(root.join("backups/claude").read_dir().unwrap().next().is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn toml_upsert_skill_config_keeps_plugins_and_quotes_windows_paths() {
        let root = crate::paths::scratch_dir("on-n-off-toml");
        let path = root.join("config.toml");
        fs::write(
            &path,
            "[plugins.\"workbench@workshop\"]\nenabled = true\n\n[projects.'E:\\\\dev\\\\on-n-off']\ntrust_level = \"trusted\"\n",
        )
        .unwrap();
        let io = ConfigIo::at(root.join("backups"));
        let skill = r"C:\Users\giova\fake-home\.agents\skills\loom-feed\SKILL.md";
        io.patch_toml_skill_enabled(AgentId::Codex, &path, skill, false)
            .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("[plugins.\"workbench@workshop\"]"), "{text}");
        assert!(text.contains("trust_level"), "{text}");
        assert!(text.contains("[[skills.config]]"), "{text}");
        assert!(text.contains("enabled = false"), "{text}");
        assert!(text.contains("loom-feed"), "{text}");
        io.patch_toml_skill_enabled(AgentId::Codex, &path, skill, true)
            .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("[[skills.config]]").count(), 1);
        assert!(text.contains("enabled = true"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn json_mcp_toggle_sets_disabled_flag_and_list() {
        let root = crate::paths::scratch_dir("on-n-off-json-mcp");
        let path = root.join(".claude.json");
        fs::write(
            &path,
            r#"{
  "numStartups": 3,
  "mcpServers": {
    "github": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"] },
    "docs": { "type": "http", "url": "https://docs.example/mcp" }
  }
}"#,
        )
        .unwrap();
        let io = ConfigIo::at(root.join("backups"));
        io.patch_json_mcp_enabled(AgentId::Claude, &path, "github", false)
            .unwrap();
        let value: JsonValue = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["numStartups"], 3);
        assert_eq!(value["mcpServers"]["docs"]["url"], "https://docs.example/mcp");
        assert_eq!(value["mcpServers"]["github"]["disabled"], true);
        assert_eq!(value["mcpServers"]["github"]["command"], "npx");
        assert_eq!(value["disabledMcpServers"], json!(["github"]));
        io.patch_json_mcp_enabled(AgentId::Claude, &path, "github", true)
            .unwrap();
        let value: JsonValue = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["mcpServers"]["github"]["disabled"], false);
        assert_eq!(value["disabledMcpServers"], json!([]));
        let err = io
            .patch_json_mcp_enabled(AgentId::Claude, &path, "missing", false)
            .expect_err("missing");
        assert!(err.message.contains("mcp server not found"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn antigravity_mcp_toggle_sets_disabled_only() {
        let root = crate::paths::scratch_dir("on-n-off-agy-mcp");
        let path = root.join("mcp_config.json");
        fs::write(
            &path,
            r#"{ "mcpServers": { "github": { "command": "npx", "args": ["-y", "gh"] } } }"#,
        )
        .unwrap();
        let io = ConfigIo::at(root.join("backups"));
        io.patch_antigravity_mcp_enabled(AgentId::Antigravity, &path, "github", false)
            .unwrap();
        let value: JsonValue = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["mcpServers"]["github"]["disabled"], true);
        assert!(value.get("disabledMcpServers").is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn toml_mcp_enable_only_flips_enabled() {
        let root = crate::paths::scratch_dir("on-n-off-toml-mcp");
        let path = root.join("config.toml");
        fs::write(
            &path,
            "[plugins.\"workbench@workshop\"]\nenabled = true\n\n[mcp_servers.github]\ncommand = \"npx\"\nargs = [\"-y\", \"@modelcontextprotocol/server-github\"]\nextra = 1\n\n[mcp_servers.docs]\nurl = \"https://docs.example/mcp\"\nenabled = false\n",
        )
        .unwrap();
        let io = ConfigIo::at(root.join("backups"));
        io.patch_toml_mcp_enabled(AgentId::Codex, &path, "github", false)
            .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("[plugins.\"workbench@workshop\"]"), "{text}");
        assert!(text.contains("extra = 1"), "{text}");
        assert!(text.contains("[mcp_servers.github]"), "{text}");
        assert!(text.contains("[mcp_servers.docs]"), "{text}");
        assert!(text.contains("enabled = false"), "{text}");
        io.patch_toml_mcp_enabled(AgentId::Codex, &path, "docs", true)
            .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("url = \"https://docs.example/mcp\""), "{text}");
        assert_eq!(text.matches("enabled = true").count(), 2, "{text}");
        let err = io
            .patch_toml_mcp_enabled(AgentId::Codex, &path, "missing", false)
            .expect_err("missing");
        assert!(err.message.contains("mcp server not found"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn toml_plugin_enable_only_flips_enabled() {
        let root = crate::paths::scratch_dir("on-n-off-toml-plugin");
        let path = root.join("config.toml");
        fs::write(
            &path,
            "[plugins.\"workbench@workshop\"]\nenabled = true\nextra = 1\n\n[plugins.\"toolkit@workshop\"]\nenabled = false\n",
        )
        .unwrap();
        let io = ConfigIo::at(root.join("backups"));
        io.patch_toml_plugin_enabled(AgentId::Codex, &path, "workbench@workshop", false)
            .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("[plugins.\"workbench@workshop\"]"));
        assert!(text.contains("[plugins.\"toolkit@workshop\"]"));
        assert!(text.contains("extra = 1"));
        assert!(text.contains("enabled = false"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unreadable_write_restores_backup() {
        let root = crate::paths::scratch_dir("on-n-off-restore");
        let path = root.join("settings.json");
        fs::write(&path, "{\"keep\":true}").unwrap();
        let io = ConfigIo::at(root.join("backups"));
        let err = io
            .force_invalid_commit(AgentId::Claude, &path)
            .expect_err("invalid commit");
        assert_eq!(err.kind, ErrorKind::Write);
        let value: JsonValue = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value, json!({"keep": true}));
        let _ = fs::remove_dir_all(root);
    }
}
