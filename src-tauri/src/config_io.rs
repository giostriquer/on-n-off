use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;
use toml_edit::DocumentMut;

use crate::backup::BackupStore;
use crate::dto::{AdapterError, AgentId};

pub struct ConfigIo {
    backups: BackupStore,
}

pub struct Backups(Vec<(PathBuf, Option<PathBuf>)>);

impl Backups {
    fn of(&self, path: &Path) -> Result<Option<&Path>, AdapterError> {
        self.0
            .iter()
            .find(|(file, _)| file == path)
            .map(|(_, backup)| backup.as_deref())
            .ok_or_else(|| {
                AdapterError::write(
                    "Not backed up before the write.",
                    Some(path.display().to_string()),
                )
            })
    }
}

type JsonEdit<'a> = Box<dyn FnOnce(&mut JsonValue) -> Result<(), AdapterError> + 'a>;
type TomlEdit<'a> = Box<dyn FnOnce(&mut DocumentMut) -> Result<(), AdapterError> + 'a>;

pub enum Edit<'a> {
    Json(JsonEdit<'a>),
    Toml(TomlEdit<'a>),
}

impl<'a> Edit<'a> {
    pub fn json(edit: impl FnOnce(&mut JsonValue) -> Result<(), AdapterError> + 'a) -> Self {
        Self::Json(Box::new(edit))
    }

    pub fn toml(edit: impl FnOnce(&mut DocumentMut) -> Result<(), AdapterError> + 'a) -> Self {
        Self::Toml(Box::new(edit))
    }
}

#[cfg(test)]
pub fn patched(agent: AgentId, path: &Path, edit: Edit<'_>) -> Result<(), AdapterError> {
    let io = ConfigIo::at(path.parent().unwrap().join("backups"));
    let backups = io.back_up(agent, &[path.to_path_buf()])?;
    io.patch(path, &backups, edit)
}

impl ConfigIo {
    pub(crate) fn patch_account_identity(
        path: &Path,
        account: Option<&JsonValue>,
    ) -> Result<(), String> {
        let original = crate::accounts::native::read_json(path)?;
        let mut next = original.clone();
        let object = next
            .as_object_mut()
            .ok_or("Malformed native account configuration.")?;
        if let Some(account) = account {
            object.insert("oauthAccount".into(), account.clone());
        } else {
            object.remove("oauthAccount");
        }
        let bytes =
            serde_json::to_vec_pretty(&next).map_err(|_| "Cannot encode account configuration.")?;
        let result = crate::accounts::vault::atomic_write(path, &bytes).and_then(|()| {
            if crate::accounts::native::read_json(path)? == next {
                Ok(())
            } else {
                Err("Account configuration validation failed.".into())
            }
        });
        if result.is_err() {
            let bytes = serde_json::to_vec_pretty(&original)
                .map_err(|_| "Cannot encode account recovery.")?;
            crate::accounts::vault::atomic_write(path,&bytes).map_err(|_|"Account configuration recovery failed. The protected activation journal has been retained.")?;
        }
        result
    }

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

    pub fn back_up(&self, agent: AgentId, files: &[PathBuf]) -> Result<Backups, AdapterError> {
        files
            .iter()
            .map(|file| Ok((file.clone(), self.backups.backup(agent, file)?)))
            .collect::<Result<Vec<_>, AdapterError>>()
            .map(Backups)
    }

    pub fn patch(
        &self,
        path: &Path,
        backups: &Backups,
        edit: Edit<'_>,
    ) -> Result<(), AdapterError> {
        let backup = backups.of(path)?;
        match edit {
            Edit::Json(edit) => self.patch_json(path, backup, edit),
            Edit::Toml(edit) => self.patch_toml(path, backup, edit),
        }
    }

    fn patch_json(
        &self,
        path: &Path,
        backup: Option<&Path>,
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
        let rendered = format!(
            "{}\n",
            serde_json::to_string_pretty(&value).map_err(|error| {
                AdapterError::write(error.to_string(), Some(path.display().to_string()))
            })?
        );
        self.commit(path, backup, &original, &rendered, |text| {
            serde_json::from_str::<JsonValue>(text).map(|_| ())
        })
    }

    fn patch_toml(
        &self,
        path: &Path,
        backup: Option<&Path>,
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
        self.commit(path, backup, &original, &rendered, |text| {
            text.parse::<DocumentMut>().map(|_| ())
        })
    }

    fn commit<E>(
        &self,
        path: &Path,
        backup: Option<&Path>,
        original: &str,
        rendered: &str,
        parse: impl Fn(&str) -> Result<(), E>,
    ) -> Result<(), AdapterError> {
        if let Err(error) = atomic_replace(path, rendered) {
            rollback(&self.backups, backup, path, original);
            return Err(error);
        }
        if parse(&fs::read_to_string(path).unwrap_or_default()).is_err() {
            rollback(&self.backups, backup, path, original);
            return Err(AdapterError::write(
                "Write failed. Restored the previous file.",
                Some(path.display().to_string()),
            ));
        }
        Ok(())
    }
}

fn read_or_empty(path: &Path) -> Result<String, AdapterError> {
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(path)
        .map_err(|error| AdapterError::parse(error.to_string(), Some(path.display().to_string())))
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
    let result = fs::rename(&tmp, path)
        .map_err(|error| AdapterError::write(error.to_string(), Some(path.display().to_string())));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests;
