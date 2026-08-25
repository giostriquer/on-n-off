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
/// rather than migrated, since the next successful read rewrites the file anyway.
pub(super) fn load(path: &Path) -> Option<GithubPrsData> {
    let stored: Stored = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    (stored.schema_version == SCHEMA_VERSION).then_some(stored.data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{
        CiState, GithubMergeQueueDto, GithubPrDto, GithubPrListDto, MergeState, Mergeability,
    };
    use crate::paths::{github_prs_path_for, scratch_dir};
    use std::fs;

    fn data() -> GithubPrsData {
        GithubPrsData {
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
                    mergeable: Mergeability::Conflicting,
                    merge_state: MergeState::Dirty,
                    merge_queue: Some(GithubMergeQueueDto { position: Some(2) }),
                    auto_merge: true,
                }],
            },
            review_requested: GithubPrListDto::default(),
            assigned: GithubPrListDto::default(),
            rate_limit: None,
        }
    }

    #[test]
    fn a_saved_read_loads_back_unchanged() {
        let home = scratch_dir("gh-snapshot-roundtrip");
        let path = github_prs_path_for(&home);
        save(&path, &data()).unwrap();
        assert_eq!(load(&path), Some(data()));
        let siblings: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(siblings, vec![std::ffi::OsString::from("prs.json")]);
        let raw = fs::read_to_string(&path).unwrap();
        assert!(
            !raw.contains("\"status\""),
            "the envelope is not persisted: {raw}"
        );
    }

    /// v0.2.0 wrote schema 1 without the merge-state fields; that file must keep loading.
    #[test]
    fn a_snapshot_from_before_the_merge_state_fields_loads_with_defaults() {
        let home = scratch_dir("gh-snapshot-v0-2-0");
        let path = github_prs_path_for(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"schemaVersion":1,"data":{"viewer":"octocat","fetchedAt":"2026-08-24T20:00:00Z","scope":["org:acme"],"mine":{"total":1,"items":[{"id":"PR_1","number":1,"title":"T","url":"https://github.com/acme/app/pull/1","repo":"acme/app","author":"octocat","isDraft":false,"ci":"success","headRef":"h","baseRef":"main","updatedAt":"2026-08-24T19:00:00Z"}]},"reviewRequested":{"total":0,"items":[]},"assigned":{"total":0,"items":[]}}}"#,
        )
        .unwrap();
        let loaded = load(&path).expect("the old snapshot still loads");
        let pr = &loaded.mine.items[0];
        assert_eq!(pr.id, "PR_1");
        assert_eq!(pr.mergeable, Mergeability::Unknown);
        assert_eq!(pr.merge_state, MergeState::Unknown);
        assert_eq!(pr.merge_queue, None);
        assert!(!pr.auto_merge);
    }

    /// The screen's TypeScript unions compare against these exact strings; a rename on either
    /// side would silently blank every merge badge.
    #[test]
    fn the_merge_fields_use_the_camel_case_names_the_screen_reads() {
        let home = scratch_dir("gh-snapshot-wire-names");
        let path = github_prs_path_for(&home);
        save(&path, &data()).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        for needle in [
            r#""mergeable":"conflicting""#,
            r#""mergeState":"dirty""#,
            r#""mergeQueue":{"position":2}"#,
            r#""autoMerge":true"#,
        ] {
            assert!(raw.contains(needle), "{needle} missing from {raw}");
        }
        let pr: GithubPrDto = serde_json::from_str(
            r#"{"id":"PR_2","number":2,"title":"T","url":"https://github.com/acme/app/pull/2","repo":"acme/app","author":"octocat","isDraft":false,"ci":"success","headRef":"h","baseRef":"main","updatedAt":"2026-08-24T19:00:00Z","mergeable":"mergeable","mergeState":"clean","mergeQueue":{},"autoMerge":false}"#,
        )
        .unwrap();
        assert_eq!(pr.mergeable, Mergeability::Mergeable);
        assert_eq!(pr.merge_state, MergeState::Clean);
        assert_eq!(
            pr.merge_queue,
            Some(GithubMergeQueueDto { position: None }),
            "queued without a known position"
        );
    }

    #[test]
    fn an_absent_or_foreign_snapshot_is_none() {
        let home = scratch_dir("gh-snapshot-absent");
        let path = github_prs_path_for(&home);
        assert_eq!(load(&path), None);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"schemaVersion":99,"data":{}}"#).unwrap();
        assert_eq!(load(&path), None);
        fs::write(&path, "{nope").unwrap();
        assert_eq!(load(&path), None);
    }
}
