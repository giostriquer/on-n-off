//! The last successful read, kept under `~/.on-n-off/github/prs.json` so the screen renders
//! before the first poll answers and keeps rendering when GitHub or `gh` is unavailable. It holds
//! PR titles and URLs only — never the token, and none of the transient envelope (status, hint).

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::dto::GithubPrsData;
use crate::usage::cache_io::atomic_write;

const SCHEMA_VERSION: u8 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    schema_version: u8,
    data: GithubPrsData,
}

pub(super) fn save(path: &Path, data: &GithubPrsData) -> Result<(), String> {
    let stored = Stored {
        schema_version: SCHEMA_VERSION,
        data: data.clone(),
    };
    let json = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
    atomic_write(path, &json).map_err(|error| format!("{}: {error}", path.display()))
}

/// `None` for an absent, unreadable, or differently-versioned file; old versions are ignored
/// rather than migrated, since the next successful read rewrites the file anyway. The merge
/// verdict is re-derived from the raw fields, so a file written by another version of the
/// classification never shows a stale one.
pub(super) fn load(path: &Path) -> Option<GithubPrsData> {
    let stored: Stored = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    if stored.schema_version != SCHEMA_VERSION {
        return None;
    }
    let mut data = stored.data;
    for pr in data
        .mine
        .items
        .iter_mut()
        .chain(data.review_requested.items.iter_mut())
        .chain(data.assigned.items.iter_mut())
    {
        pr.merge_kind = super::merge::classify(pr);
    }
    Some(data)
}

#[cfg(test)]
mod tests;
