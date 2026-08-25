//! The one GraphQL document the screen needs, and the search strings that feed it. Four searches
//! ride in a single request; without a `contexts` connection the whole thing costs about two
//! rate-limit points, so a 60 s poll spends ~120 of the 5 000 points an hour GitHub grants. The
//! merge-state fields are scalars and single objects, so they add nothing to that cost.

use serde_json::{json, Value};

pub(super) const GRAPHQL_URL: &str = "https://api.github.com/graphql";
/// Items per list; the DTO's `total` still reports the full match count.
pub(super) const PAGE_SIZE: u32 = 50;

const DOCUMENT: &str = "\
query($mine: String!, $review: String!, $direct: String!, $assigned: String!, $first: Int!) {
  viewer { login }
  rateLimit { remaining resetAt }
  mine: search(type: ISSUE, query: $mine, first: $first) { issueCount nodes { ...pr } }
  review: search(type: ISSUE, query: $review, first: $first) { issueCount nodes { ...pr } }
  direct: search(type: ISSUE, query: $direct, first: $first) { issueCount nodes { ... on PullRequest { id } } }
  assigned: search(type: ISSUE, query: $assigned, first: $first) { issueCount nodes { ...pr } }
}
fragment pr on PullRequest {
  id number title url isDraft updatedAt headRefName baseRefName reviewDecision
  mergeable mergeStateStatus mergeQueueEntry { position } autoMergeRequest { enabledAt }
  repository { nameWithOwner }
  author { login }
  commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
}";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Search {
    /// Open PRs the user authored, narrowed by the configured scopes.
    Mine,
    /// Open PRs asking for the user's review, directly or through one of their teams.
    ReviewRequested,
    /// The subset of `ReviewRequested` that named the user; only used to tag rows.
    DirectReviewRequested,
    /// Open PRs with the user in the assignee field.
    Assigned,
}

pub(super) fn search_query(search: Search, scopes: &[String]) -> String {
    let base = match search {
        Search::Mine => "is:pr is:open author:@me",
        Search::ReviewRequested => "is:pr is:open review-requested:@me",
        Search::DirectReviewRequested => "is:pr is:open user-review-requested:@me",
        Search::Assigned => "is:pr is:open assignee:@me",
    };
    if search != Search::Mine || scopes.is_empty() {
        return base.to_string();
    }
    let mut query = base.to_string();
    for scope in scopes {
        query.push(' ');
        query.push_str(scope);
    }
    query
}

pub(super) fn request_body(scopes: &[String]) -> Value {
    json!({
        "query": DOCUMENT,
        "variables": {
            "mine": search_query(Search::Mine, scopes),
            "review": search_query(Search::ReviewRequested, scopes),
            "direct": search_query(Search::DirectReviewRequested, scopes),
            "assigned": search_query(Search::Assigned, scopes),
            "first": PAGE_SIZE,
        }
    })
}

#[cfg(test)]
mod tests {
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
    }
}
