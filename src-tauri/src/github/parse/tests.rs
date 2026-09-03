use super::*;
use crate::dto::{
    CiState, GithubMergeQueueDto, MergeKind, MergeState, Mergeability, ReviewDecision,
    ReviewRequestKind,
};
use crate::github::fixtures::REPLY;
use serde_json::json;

fn reply() -> Value {
    serde_json::from_str(REPLY).unwrap()
}

#[test]
fn the_fixture_parses_into_the_three_lists() {
    let parsed = parse(&reply()).unwrap();
    assert_eq!(parsed.data.viewer.as_deref(), Some("octocat"));
    assert_eq!(parsed.data.rate_limit.as_ref().unwrap().remaining, 4998);
    assert_eq!(
        parsed.data.rate_limit.as_ref().unwrap().reset_at,
        "2026-08-24T23:00:00Z"
    );
    assert!(parsed.warnings.is_empty());

    assert_eq!(parsed.data.mine.total, 1);
    let mine = &parsed.data.mine.items[0];
    assert_eq!(mine.id, "PR_mine1");
    assert_eq!(mine.number, 41);
    assert_eq!(mine.title, "Add the thing");
    assert_eq!(mine.url, "https://github.com/acme/app/pull/41");
    assert_eq!(mine.repo, "acme/app");
    assert_eq!(mine.author, "octocat");
    assert!(!mine.is_draft);
    assert_eq!(mine.review_decision, Some(ReviewDecision::ReviewRequired));
    assert_eq!(mine.ci, CiState::Failure);
    assert_eq!(mine.head_ref, "feat/thing");
    assert_eq!(mine.base_ref, "main");
    assert_eq!(mine.updated_at, "2026-08-24T20:00:00Z");
    assert_eq!(mine.review_request, None);
    assert_eq!(mine.mergeable, Mergeability::Mergeable);
    assert_eq!(mine.merge_state, MergeState::Blocked);
    assert_eq!(mine.merge_queue, None);
    assert!(!mine.auto_merge);
    assert_eq!(
        mine.merge_kind, None,
        "blocked with a review required is the default state, not a badge"
    );

    assert_eq!(parsed.data.review_requested.total, 2);
    let [direct, team] = parsed.data.review_requested.items.as_slice() else {
        panic!("{:?}", parsed.data.review_requested);
    };
    assert_eq!(direct.review_request, Some(ReviewRequestKind::Direct));
    assert!(direct.is_draft);
    assert_eq!(direct.ci, CiState::Pending);
    assert_eq!(direct.review_decision, None);
    // A draft's state is `DRAFT` whatever the merge would do; only `mergeable` carries the
    // conflicts, which is why both fields ride along.
    assert_eq!(direct.mergeable, Mergeability::Conflicting);
    assert_eq!(direct.merge_state, MergeState::Draft);
    assert_eq!(direct.merge_kind, Some(MergeKind::Conflicts));
    assert_eq!(team.review_request, Some(ReviewRequestKind::Team));
    assert_eq!(team.ci, CiState::None, "a null rollup means no checks");
    assert_eq!(team.author, "", "a deleted author is not an error");
    assert_eq!(team.mergeable, Mergeability::Conflicting);
    assert_eq!(team.merge_state, MergeState::Dirty);
    assert!(team.auto_merge);
    assert_eq!(team.merge_kind, Some(MergeKind::Conflicts));

    assert_eq!(parsed.data.assigned.total, 0);
    assert!(parsed.data.assigned.items.is_empty());
}

#[test]
fn every_rollup_state_maps_and_unknown_ones_fall_back_to_none() {
    for (state, expected) in [
        (json!("SUCCESS"), CiState::Success),
        (json!("FAILURE"), CiState::Failure),
        (json!("ERROR"), CiState::Error),
        (json!("PENDING"), CiState::Pending),
        (json!("EXPECTED"), CiState::Pending),
        (json!("SOMETHING_NEW"), CiState::None),
        (json!(null), CiState::None),
    ] {
        let mut value = reply();
        value["data"]["mine"]["nodes"][0]["commits"]["nodes"][0]["commit"]["statusCheckRollup"] =
            if state.is_null() {
                json!(null)
            } else {
                json!({ "state": state })
            };
        let parsed = parse(&value).unwrap();
        assert_eq!(parsed.data.mine.items[0].ci, expected, "{state}");
    }
}

#[test]
fn every_mergeable_value_maps_and_unknown_ones_fall_back_to_unknown() {
    for (value, expected) in [
        (json!("MERGEABLE"), Mergeability::Mergeable),
        (json!("CONFLICTING"), Mergeability::Conflicting),
        (json!("UNKNOWN"), Mergeability::Unknown),
        (json!("SOMETHING_NEW"), Mergeability::Unknown),
        (json!(null), Mergeability::Unknown),
    ] {
        let mut reply = reply();
        reply["data"]["mine"]["nodes"][0]["mergeable"] = value.clone();
        assert_eq!(
            parse(&reply).unwrap().data.mine.items[0].mergeable,
            expected,
            "{value}"
        );
    }
}

#[test]
fn every_merge_state_maps_and_unknown_ones_fall_back_to_unknown() {
    for (value, expected) in [
        (json!("CLEAN"), MergeState::Clean),
        (json!("HAS_HOOKS"), MergeState::Clean),
        (json!("UNSTABLE"), MergeState::Unstable),
        (json!("BLOCKED"), MergeState::Blocked),
        (json!("BEHIND"), MergeState::Behind),
        (json!("DIRTY"), MergeState::Dirty),
        (json!("DRAFT"), MergeState::Draft),
        (json!("UNKNOWN"), MergeState::Unknown),
        (json!("SOMETHING_NEW"), MergeState::Unknown),
        (json!(null), MergeState::Unknown),
    ] {
        let mut reply = reply();
        reply["data"]["mine"]["nodes"][0]["mergeStateStatus"] = value.clone();
        assert_eq!(
            parse(&reply).unwrap().data.mine.items[0].merge_state,
            expected,
            "{value}"
        );
    }
}

#[test]
fn a_merge_queue_entry_keeps_its_position_when_there_is_one() {
    for (value, expected) in [
        (json!(null), None),
        (
            json!({ "position": 3 }),
            Some(GithubMergeQueueDto { position: Some(3) }),
        ),
        (
            json!({ "position": null }),
            Some(GithubMergeQueueDto { position: None }),
        ),
        (json!({}), Some(GithubMergeQueueDto { position: None })),
    ] {
        let mut reply = reply();
        reply["data"]["mine"]["nodes"][0]["mergeQueueEntry"] = value.clone();
        assert_eq!(
            parse(&reply).unwrap().data.mine.items[0].merge_queue,
            expected,
            "{value}"
        );
    }
    let mut reply = reply();
    reply["data"]["mine"]["nodes"][0]
        .as_object_mut()
        .unwrap()
        .remove("mergeQueueEntry");
    assert_eq!(parse(&reply).unwrap().data.mine.items[0].merge_queue, None);
}

#[test]
fn auto_merge_is_on_exactly_when_github_reports_a_request() {
    let mut reply = reply();
    reply["data"]["mine"]["nodes"][0]["autoMergeRequest"] =
        json!({ "enabledAt": "2026-08-24T17:00:00Z" });
    assert!(parse(&reply).unwrap().data.mine.items[0].auto_merge);
    reply["data"]["mine"]["nodes"][0]["autoMergeRequest"] = json!({});
    assert!(parse(&reply).unwrap().data.mine.items[0].auto_merge);
    reply["data"]["mine"]["nodes"][0]["autoMergeRequest"] = json!(null);
    assert!(!parse(&reply).unwrap().data.mine.items[0].auto_merge);
}

#[test]
fn an_unknown_review_decision_is_dropped_not_fatal() {
    let mut value = reply();
    value["data"]["mine"]["nodes"][0]["reviewDecision"] = json!("BRAND_NEW");
    assert_eq!(
        parse(&value).unwrap().data.mine.items[0].review_decision,
        None
    );
}

#[test]
fn the_total_is_the_search_count_not_the_page_size() {
    let mut value = reply();
    value["data"]["mine"]["issueCount"] = json!(137);
    let parsed = parse(&value).unwrap();
    assert_eq!(parsed.data.mine.total, 137);
    assert_eq!(parsed.data.mine.items.len(), 1);
}

#[test]
fn nodes_without_a_pull_request_shape_are_skipped() {
    let mut value = reply();
    value["data"]["assigned"]["nodes"] = json!([null, {}, { "id": "x", "number": "nope" }]);
    assert!(parse(&value).unwrap().data.assigned.items.is_empty());
}

#[test]
fn errors_next_to_data_become_warnings() {
    let mut value = reply();
    value["errors"] = json!([{ "message": "Field 'x' is deprecated" }]);
    let parsed = parse(&value).unwrap();
    assert_eq!(parsed.data.mine.items.len(), 1);
    assert_eq!(parsed.warnings, vec!["Field 'x' is deprecated".to_string()]);
}

#[test]
fn errors_without_data_fail_with_the_first_message() {
    let value = json!({ "data": null, "errors": [{ "message": "Bad query" }, { "message": "x" }] });
    assert_eq!(parse(&value).unwrap_err(), "Bad query");
    assert_eq!(parse(&json!({})).unwrap_err(), "reply has no data");
}

#[test]
fn a_missing_search_count_falls_back_to_the_page_and_missing_tagging_means_team() {
    let mut value = reply();
    value["data"]["mine"]
        .as_object_mut()
        .unwrap()
        .remove("issueCount");
    value["data"].as_object_mut().unwrap().remove("direct");
    value["data"]["mine"]["nodes"][0]["commits"]["nodes"] = json!([]);
    let parsed = parse(&value).unwrap();
    assert_eq!(parsed.data.mine.total, 1);
    assert_eq!(
        parsed.data.mine.items[0].ci,
        CiState::None,
        "no commits means no checks"
    );
    assert!(parsed
        .data
        .review_requested
        .items
        .iter()
        .all(|item| item.review_request == Some(ReviewRequestKind::Team)));
}

#[test]
fn a_missing_rate_limit_or_viewer_is_tolerated() {
    let mut value = reply();
    value["data"]["rateLimit"] = json!(null);
    value["data"]["viewer"] = json!(null);
    let parsed = parse(&value).unwrap();
    assert_eq!(parsed.data.rate_limit, None);
    assert_eq!(parsed.data.viewer, None);
}
