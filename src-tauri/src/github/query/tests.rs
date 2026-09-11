use super::*;

#[test]
fn mine_is_scoped_and_the_other_searches_are_not() {
    let scopes = vec!["org:acme".to_string(), "repo:me/tool".to_string()];
    assert_eq!(
        search_query(Search::Mine, &scopes),
        "is:pr is:open author:@me org:acme repo:me/tool"
    );
    assert_eq!(search_query(Search::Mine, &[]), "is:pr is:open author:@me");
    assert_eq!(
        search_query(Search::ReviewRequested, &scopes),
        "is:pr is:open review-requested:@me"
    );
    assert_eq!(
        search_query(Search::DirectReviewRequested, &scopes),
        "is:pr is:open user-review-requested:@me"
    );
    assert_eq!(
        search_query(Search::Assigned, &scopes),
        "is:pr is:open assignee:@me"
    );
}

#[test]
fn merged_is_scoped_like_mine_and_reads_the_latest_first() {
    let scopes = vec!["org:acme".to_string()];
    assert_eq!(
        search_query(Search::Merged, &scopes),
        "is:pr is:merged author:@me sort:updated-desc org:acme"
    );
    assert_eq!(
        search_query(Search::Merged, &[]),
        "is:pr is:merged author:@me sort:updated-desc"
    );
}

#[test]
fn the_request_body_carries_the_document_and_every_search_string() {
    let body = request_body(&["org:acme".to_string()]);
    let document = body["query"].as_str().unwrap();
    for needle in [
        "viewer { login }",
        "rateLimit { remaining resetAt }",
        "statusCheckRollup { state }",
        "reviewDecision",
        "mergeable",
        "mergeStateStatus",
        "mergeQueueEntry { position }",
        "autoMergeRequest { enabledAt }",
        "$first",
    ] {
        assert!(document.contains(needle), "{needle}");
    }
    assert!(
        !document.contains("contexts("),
        "check details would multiply the query cost"
    );
    assert_eq!(body["variables"]["first"], PAGE_SIZE);
    assert_eq!(
        body["variables"]["mine"],
        "is:pr is:open author:@me org:acme"
    );
    assert_eq!(
        body["variables"]["review"],
        "is:pr is:open review-requested:@me"
    );
    assert_eq!(
        body["variables"]["direct"],
        "is:pr is:open user-review-requested:@me"
    );
    assert_eq!(body["variables"]["assigned"], "is:pr is:open assignee:@me");
    assert_eq!(
        body["variables"]["merged"],
        "is:pr is:merged author:@me sort:updated-desc org:acme"
    );
    assert_eq!(body["variables"]["recent"], RECENT_MERGES);
    assert!(document.contains("merged: search("));
}
