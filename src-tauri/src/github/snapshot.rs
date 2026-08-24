//! The last successful read, kept under `~/.on-n-off/github/prs.json` so the screen renders
//! before the first poll answers and keeps rendering when GitHub or `gh` is unavailable. It holds
//! PR titles and URLs only — never the token.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::dto::GithubPrsDto;
use crate::usage::cache_io::atomic_write;

const SCHEMA_VERSION: u8 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    schema_version: u8,
    prs: GithubPrsDto,
}

pub(super) fn save(path: &Path, prs: &GithubPrsDto) -> Result<(), String> {
    let stored = Stored {
        schema_version: SCHEMA_VERSION,
        prs: prs.clone(),
    };
    let json = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
    atomic_write(path, &json).map_err(|error| format!("{}: {error}", path.display()))
}

/// `None` for an absent, unreadable, or differently-versioned file; old versions are ignored
/// rather than migrated, since the next successful read rewrites the file anyway.
pub(super) fn load(path: &Path) -> Option<GithubPrsDto> {
    let stored: Stored = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    (stored.schema_version == SCHEMA_VERSION).then_some(stored.prs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{CiState, GithubPrDto, GithubPrListDto, GithubStatus};
    use crate::paths::{github_prs_path_for, scratch_dir};
    use std::fs;

    fn dto() -> GithubPrsDto {
        GithubPrsDto {
            status: GithubStatus::Ok,
            hint: None,
            stale: false,
            viewer: Some("octocat".into()),
            fetched_at: Some("2026-08-24T20:00:00Z".into()),
            scope: vec!["org:acme".into()],
            mine: GithubPrListDto {
                total: 1,
                items: vec![GithubPrDto {
                    id: "PR_1".into(),
                    number: 1,
                    title: "T".into(),
                    url: "https://github.com/acme/app/pull/1".into(),
                    repo: "acme/app".into(),
                    author: "octocat".into(),
                    is_draft: false,
                    review_decision: None,
                    ci: CiState::Success,
                    head_ref: "h".into(),
                    base_ref: "main".into(),
                    updated_at: "2026-08-24T19:00:00Z".into(),
                    review_request: None,
                }],
            },
            review_requested: GithubPrListDto::default(),
            assigned: GithubPrListDto::default(),
            rate_limit: None,
            warnings: vec!["w".into()],
        }
    }

    #[test]
    fn a_saved_read_loads_back_unchanged() {
        let home = scratch_dir("gh-snapshot-roundtrip");
        let path = github_prs_path_for(&home);
        save(&path, &dto()).unwrap();
        assert_eq!(load(&path), Some(dto()));
        let siblings: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(siblings, vec![std::ffi::OsString::from("prs.json")]);
        let raw = fs::read_to_string(&path).unwrap();
        assert!(
            !raw.contains("gho_") && !raw.contains("Authorization"),
            "{raw}"
        );
    }

    #[test]
    fn an_absent_or_foreign_snapshot_is_none() {
        let home = scratch_dir("gh-snapshot-absent");
        let path = github_prs_path_for(&home);
        assert_eq!(load(&path), None);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"schemaVersion":99,"prs":{}}"#).unwrap();
        assert_eq!(load(&path), None);
        fs::write(&path, "{nope").unwrap();
        assert_eq!(load(&path), None);
    }
}
