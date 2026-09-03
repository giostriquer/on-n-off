use super::*;
use crate::dto::{
    CiState, GithubMergeQueueDto, GithubPrDto, GithubPrListDto, MergeKind, MergeState, Mergeability,
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
                merge_kind: Some(MergeKind::Conflicts),
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
    assert_eq!(pr.merge_kind, None);
}

#[test]
fn a_loaded_snapshot_re_derives_the_merge_verdict_from_its_raw_fields() {
    let home = scratch_dir("gh-snapshot-rederive");
    let path = github_prs_path_for(&home);
    let mut stale = data();
    // A file that says "ready" beside raw fields that mean conflicts (as an older or newer
    // classification could) loads with the verdict this version draws from the raw fields.
    stale.mine.items[0].merge_kind = Some(MergeKind::Ready);
    save(&path, &stale).unwrap();
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains(r#""mergeKind":"ready""#));
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.mine.items[0].merge_kind, Some(MergeKind::Conflicts));
}

/// The raw fields and the verdict are pinned on the wire so a `prs.json` written by this
/// version keeps loading in later ones (`mergeKind` is also what the screen reads).
#[test]
fn the_merge_fields_keep_their_camel_case_wire_names() {
    let home = scratch_dir("gh-snapshot-wire-names");
    let path = github_prs_path_for(&home);
    save(&path, &data()).unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    for needle in [
        r#""mergeable":"conflicting""#,
        r#""mergeState":"dirty""#,
        r#""mergeQueue":{"position":2}"#,
        r#""autoMerge":true"#,
        r#""mergeKind":"conflicts""#,
    ] {
        assert!(raw.contains(needle), "{needle} missing from {raw}");
    }
    let pr: GithubPrDto = serde_json::from_str(
        r#"{"id":"PR_2","number":2,"title":"T","url":"https://github.com/acme/app/pull/2","repo":"acme/app","author":"octocat","isDraft":false,"ci":"success","headRef":"h","baseRef":"main","updatedAt":"2026-08-24T19:00:00Z","mergeable":"mergeable","mergeState":"clean","mergeQueue":{},"autoMerge":false,"mergeKind":"autoMerge"}"#,
    )
    .unwrap();
    assert_eq!(pr.mergeable, Mergeability::Mergeable);
    assert_eq!(pr.merge_state, MergeState::Clean);
    assert_eq!(pr.merge_kind, Some(MergeKind::AutoMerge));
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
