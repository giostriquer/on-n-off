use serde_json::{json, Value};

pub(super) const GRAPHQL_URL: &str = "https://api.github.com/graphql";
pub(super) const PAGE_SIZE: u32 = 50;
pub(super) const RECENT_MERGES: u32 = 10;

const DOCUMENT: &str = "\
query($mine: String!, $review: String!, $direct: String!, $assigned: String!, $merged: String!, $first: Int!, $recent: Int!) {
  viewer { login }
  rateLimit { remaining resetAt }
  mine: search(type: ISSUE, query: $mine, first: $first) { issueCount nodes { ...pr } }
  review: search(type: ISSUE, query: $review, first: $first) { issueCount nodes { ...pr } }
  direct: search(type: ISSUE, query: $direct, first: $first) { issueCount nodes { ... on PullRequest { id } } }
  assigned: search(type: ISSUE, query: $assigned, first: $first) { issueCount nodes { ...pr } }
  merged: search(type: ISSUE, query: $merged, first: $recent) { issueCount nodes { ...pr } }
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
    Mine,
    ReviewRequested,
    DirectReviewRequested,
    Assigned,
    Merged,
}

impl Search {
    fn is_scoped(self) -> bool {
        matches!(self, Self::Mine | Self::Merged)
    }
}

pub(super) fn search_query(search: Search, scopes: &[String]) -> String {
    let base = match search {
        Search::Mine => "is:pr is:open author:@me",
        Search::ReviewRequested => "is:pr is:open review-requested:@me",
        Search::DirectReviewRequested => "is:pr is:open user-review-requested:@me",
        Search::Assigned => "is:pr is:open assignee:@me",
        Search::Merged => "is:pr is:merged author:@me sort:updated-desc",
    };
    if !search.is_scoped() || scopes.is_empty() {
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
            "merged": search_query(Search::Merged, scopes),
            "first": PAGE_SIZE,
            "recent": RECENT_MERGES,
        }
    })
}

#[cfg(test)]
mod tests;
