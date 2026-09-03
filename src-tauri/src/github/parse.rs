//! GraphQL reply → lists. Lenient by design: a node that is not PR-shaped is skipped, an enum
//! value this version does not know collapses to the field's "nothing known" value (`None` for
//! the review decision, `CiState::None` for the rollup, `Unknown` for the merge fields), and
//! `errors[]` next to usable `data` are warnings rather than a failure.

use std::collections::HashSet;

use serde_json::Value;

use crate::dto::{
    CiState, GithubMergeQueueDto, GithubPrDto, GithubPrListDto, GithubPrsData, GithubRateLimitDto,
    MergeState, Mergeability, ReviewDecision, ReviewRequestKind,
};

/// The lists and viewer of one reply (`fetched_at` and `scope` are the reader's to fill in),
/// plus any error messages GitHub attached to otherwise usable data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedPrs {
    pub(super) data: GithubPrsData,
    pub(super) warnings: Vec<String>,
}

/// `Err` carries the first GraphQL error message when the reply has no usable `data`.
pub(super) fn parse(reply: &Value) -> Result<ParsedPrs, String> {
    let warnings: Vec<String> = reply["errors"]
        .as_array()
        .map(|errors| {
            errors
                .iter()
                .filter_map(|error| error["message"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let data = &reply["data"];
    if !data.is_object() {
        return Err(warnings
            .into_iter()
            .next()
            .unwrap_or_else(|| "reply has no data".to_string()));
    }
    let direct: HashSet<String> = data["direct"]["nodes"]
        .as_array()
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|node| node["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let mut review_requested = list(&data["review"]);
    for item in &mut review_requested.items {
        item.review_request = Some(if direct.contains(&item.id) {
            ReviewRequestKind::Direct
        } else {
            ReviewRequestKind::Team
        });
    }
    Ok(ParsedPrs {
        data: GithubPrsData {
            viewer: data["viewer"]["login"].as_str().map(str::to_string),
            fetched_at: None,
            scope: Vec::new(),
            mine: list(&data["mine"]),
            review_requested,
            assigned: list(&data["assigned"]),
            rate_limit: rate_limit(&data["rateLimit"]),
        },
        warnings,
    })
}

fn list(search: &Value) -> GithubPrListDto {
    let items: Vec<GithubPrDto> = search["nodes"]
        .as_array()
        .map(|nodes| nodes.iter().filter_map(pull_request).collect())
        .unwrap_or_default();
    GithubPrListDto {
        total: search["issueCount"].as_u64().unwrap_or(items.len() as u64),
        items,
    }
}

fn pull_request(node: &Value) -> Option<GithubPrDto> {
    let text = |key: &str| node[key].as_str().map(str::to_string);
    let mut pr = GithubPrDto {
        id: text("id")?,
        number: node["number"].as_u64()?,
        title: text("title")?,
        url: text("url")?,
        repo: node["repository"]["nameWithOwner"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        author: node["author"]["login"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        is_draft: node["isDraft"].as_bool().unwrap_or(false),
        review_decision: review_decision(node["reviewDecision"].as_str()),
        ci: ci_state(node["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"].as_str()),
        head_ref: text("headRefName").unwrap_or_default(),
        base_ref: text("baseRefName").unwrap_or_default(),
        updated_at: text("updatedAt").unwrap_or_default(),
        review_request: None,
        mergeable: mergeable(node["mergeable"].as_str()),
        merge_state: merge_state(node["mergeStateStatus"].as_str()),
        merge_queue: merge_queue(&node["mergeQueueEntry"]),
        auto_merge: node["autoMergeRequest"].is_object(),
        merge_kind: None,
    };
    pr.merge_kind = super::merge::classify(&pr);
    Some(pr)
}

fn mergeable(value: Option<&str>) -> Mergeability {
    match value {
        Some("MERGEABLE") => Mergeability::Mergeable,
        Some("CONFLICTING") => Mergeability::Conflicting,
        _ => Mergeability::Unknown,
    }
}

fn merge_state(value: Option<&str>) -> MergeState {
    match value {
        Some("CLEAN" | "HAS_HOOKS") => MergeState::Clean,
        Some("UNSTABLE") => MergeState::Unstable,
        Some("BLOCKED") => MergeState::Blocked,
        Some("BEHIND") => MergeState::Behind,
        Some("DIRTY") => MergeState::Dirty,
        Some("DRAFT") => MergeState::Draft,
        _ => MergeState::Unknown,
    }
}

/// A queue entry is an object (even an empty one) while the pull request is queued.
fn merge_queue(value: &Value) -> Option<GithubMergeQueueDto> {
    value.is_object().then(|| GithubMergeQueueDto {
        position: value["position"].as_u64(),
    })
}

fn review_decision(value: Option<&str>) -> Option<ReviewDecision> {
    match value {
        Some("APPROVED") => Some(ReviewDecision::Approved),
        Some("CHANGES_REQUESTED") => Some(ReviewDecision::ChangesRequested),
        Some("REVIEW_REQUIRED") => Some(ReviewDecision::ReviewRequired),
        _ => None,
    }
}

fn ci_state(state: Option<&str>) -> CiState {
    match state {
        Some("SUCCESS") => CiState::Success,
        Some("FAILURE") => CiState::Failure,
        Some("ERROR") => CiState::Error,
        Some("PENDING" | "EXPECTED") => CiState::Pending,
        _ => CiState::None,
    }
}

fn rate_limit(value: &Value) -> Option<GithubRateLimitDto> {
    Some(GithubRateLimitDto {
        remaining: value["remaining"].as_u64()?,
        reset_at: value["resetAt"].as_str()?.to_string(),
    })
}

#[cfg(test)]
mod tests;
