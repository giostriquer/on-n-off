//! Copying an item out of a tarball onto disk: hashing, staging, atomic placement.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::fetch::Tarball;
use crate::dto::{AdapterError, ItemKind};

/// Files of one item, keyed by path relative to the item (a skill folder's contents, or the
/// agent file under its own file name).
pub type ItemFiles = BTreeMap<String, Vec<u8>>;

/// Normalises a repository-relative path: `\` → `/`, no leading `./`, no trailing `/`.
/// Rejects anything that could leave the repository root.
pub fn normalize_upstream_path(path: &str) -> Result<String, AdapterError> {
    let unified = path.trim().replace('\\', "/");
    let mut parts = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                return Err(AdapterError::parse(
                    format!("path escapes the repository: {path}"),
                    None,
                ))
            }
            other if other.contains(':') => {
                return Err(AdapterError::parse(
                    format!("path is not repository-relative: {path}"),
                    None,
                ))
            }
            other => parts.push(other),
        }
    }
    Ok(parts.join("/"))
}

pub fn item_files(
    tarball: &Tarball,
    upstream_path: &str,
    kind: ItemKind,
) -> Result<ItemFiles, AdapterError> {
    let path = normalize_upstream_path(upstream_path)?;
    match kind {
        ItemKind::Skill => {
            let files: ItemFiles = tarball
                .subtree(&path)
                .into_iter()
                .map(|(k, v)| (k, v.to_vec()))
                .collect();
            if files.is_empty() {
                return Err(AdapterError::message(format!(
                    "{path} is not in upstream at {}",
                    short(&tarball.commit_sha)
                )));
            }
            Ok(files)
        }
        ItemKind::Agent => {
            let bytes = tarball.files.get(&path).ok_or_else(|| {
                AdapterError::message(format!(
                    "{path} is not in upstream at {}",
                    short(&tarball.commit_sha)
                ))
            })?;
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let mut files = ItemFiles::new();
            files.insert(name, bytes.clone());
            Ok(files)
        }
    }
}

pub fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn hash_files(files: &ItemFiles) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|(path, bytes)| (path.clone(), hash_bytes(bytes)))
        .collect()
}

/// Hashes what is on disk for an item, `None` when the target is gone.
pub fn hash_tree_on_disk(
    target: &Path,
    kind: ItemKind,
) -> Result<Option<BTreeMap<String, String>>, AdapterError> {
    match kind {
        ItemKind::Agent => {
            if !target.is_file() {
                return Ok(None);
            }
            let bytes = fs::read(target).map_err(|error| io_error(error, target))?;
            let name = target
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut map = BTreeMap::new();
            map.insert(name, hash_bytes(&bytes));
            Ok(Some(map))
        }
        ItemKind::Skill => {
            if !target.is_dir() {
                return Ok(None);
            }
            let mut map = BTreeMap::new();
            walk(target, target, &mut map)?;
            Ok(Some(map))
        }
    }
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) -> Result<(), AdapterError> {
    let entries = fs::read_dir(dir).map_err(|error| io_error(error, dir))?;
    for entry in entries {
        let entry = entry.map_err(|error| io_error(error, dir))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| io_error(error, &path))?;
        if file_type.is_dir() {
            walk(root, &path, out)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::read(&path).map_err(|error| io_error(error, &path))?;
            out.insert(relative, hash_bytes(&bytes));
        }
    }
    Ok(())
}

/// Writes `files` as `dest` (a folder for skills, a single file for agents), replacing any
/// previous copy atomically: everything is staged in a sibling, then swapped in with a rename.
pub fn place(dest: &Path, kind: ItemKind, files: &ItemFiles) -> Result<(), AdapterError> {
    place_with(dest, kind, files, || Ok(()))
}

pub fn place_with(
    dest: &Path,
    kind: ItemKind,
    files: &ItemFiles,
    before_replace: impl FnOnce() -> io::Result<()>,
) -> Result<(), AdapterError> {
    let parent = dest
        .parent()
        .ok_or_else(|| AdapterError::message("item destination has no parent folder"))?;
    fs::create_dir_all(parent).map_err(|error| io_error(error, parent))?;
    let staging = temp_sibling(dest);
    let _ = remove_any(&staging);
    let result = (|| -> io::Result<()> {
        match kind {
            ItemKind::Agent => {
                let (_, bytes) = files
                    .iter()
                    .next()
                    .ok_or_else(|| io::Error::other("agent item has no file"))?;
                fs::write(&staging, bytes)?;
            }
            ItemKind::Skill => {
                fs::create_dir_all(&staging)?;
                for (relative, bytes) in files {
                    let target = staging.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
                    if let Some(dir) = target.parent() {
                        fs::create_dir_all(dir)?;
                    }
                    fs::write(&target, bytes)?;
                }
            }
        }
        before_replace()?;
        swap_in(&staging, dest)
    })();
    if result.is_err() {
        let _ = remove_any(&staging);
    }
    result.map_err(|error| io_error(error, dest))
}

/// Replaces `dest` with `staging`. The old copy is moved aside first so a failed rename can put
/// it back; the aside copy is removed only once the new one is in place.
fn swap_in(staging: &Path, dest: &Path) -> io::Result<()> {
    let existed = dest.exists();
    let aside = aside_sibling(dest);
    if existed {
        let _ = remove_any(&aside);
        fs::rename(dest, &aside)?;
    }
    match fs::rename(staging, dest) {
        Ok(()) => {
            if existed {
                let _ = remove_any(&aside);
            }
            Ok(())
        }
        Err(error) => {
            if existed {
                let _ = fs::rename(&aside, dest);
            }
            Err(error)
        }
    }
}

pub fn remove_any(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn temp_sibling(path: &Path) -> PathBuf {
    sibling(path, "tmp")
}

fn aside_sibling(path: &Path) -> PathBuf {
    sibling(path, "old")
}

fn sibling(path: &Path, tag: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{}.{tag}-on-n-off", std::process::id()));
    PathBuf::from(name)
}

pub fn io_error(error: io::Error, path: &Path) -> AdapterError {
    AdapterError::write(error.to_string(), Some(path.display().to_string()))
}
