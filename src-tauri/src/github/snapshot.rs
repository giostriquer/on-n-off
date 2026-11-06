use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::merge::KnownAnswers;
use crate::dto::GithubPrsData;
use crate::usage::cache_io::atomic_write;

const SCHEMA_VERSION: u8 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    schema_version: u8,
    data: GithubPrsData,
    #[serde(default)]
    known_answers: Option<KnownAnswers>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub(super) data: GithubPrsData,
    pub(super) known_answers: Option<KnownAnswers>,
}

pub(super) fn save(
    path: &Path,
    data: &GithubPrsData,
    known_answers: &KnownAnswers,
) -> Result<(), String> {
    let stored = Stored {
        schema_version: SCHEMA_VERSION,
        data: data.clone(),
        known_answers: Some(known_answers.clone()),
    };
    let json = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
    atomic_write(path, &json).map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn load(path: &Path) -> Option<Snapshot> {
    let stored: Stored = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    (stored.schema_version == SCHEMA_VERSION).then_some(Snapshot {
        data: stored.data,
        known_answers: stored.known_answers,
    })
}

#[cfg(test)]
mod tests;
