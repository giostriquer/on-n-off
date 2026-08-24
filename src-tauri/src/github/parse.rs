//! GraphQL reply → lists. Lenient by design: a node that is not PR-shaped is skipped, an enum
//! value this version does not know becomes `None`, and `errors[]` next to usable `data` are
//! warnings rather than a failure.

use std::collections::HashSet;

use serde_json::Value;

use crate::dto::{CiState, GithubPrDto, GithubPrListDto, GithubRateLimitDto, ReviewRequestKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedPrs {
    pub(super) viewer: Option<String>,
    pub(super) mine: GithubPrListDto,
    pub(super) review_requested: GithubPrListDto,
    pub(super) assigned: GithubPrListDto,
    pub(super) rate_limit: Option<GithubRateLimitDto>,
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
        viewer: data["viewer"]["login"].as_str().map(str::to_string),
        mine: list(&data["mine"]),
        review_requested,
        assigned: list(&data["assigned"]),
        rate_limit: rate_limit(&data["rateLimit"]),
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
    Some(GithubPrDto {
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
        review_decision: node["reviewDecision"]
            .as_str()
            .filter(|value| matches!(*value, "APPROVED" | "CHANGES_REQUESTED" | "REVIEW_REQUIRED"))
            .map(str::to_string),
        ci: ci_state(node["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"].as_str()),
        head_ref: text("headRefName").unwrap_or_default(),
        base_ref: text("baseRefName").unwrap_or_default(),
        updated_at: text("updatedAt").unwrap_or_default(),
        review_request: None,
    })
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
mod tests {
    use super::*;
    use crate::dto::{CiState, ReviewRequestKind};
    use crate::github::fixtures::REPLY;
    use serde_json::json;

    fn reply() -> Value {
        serde_json::from_str(REPLY).unwrap()
    }

    #[test]
    fn the_fixture_parses_into_the_three_lists() {
        let parsed = parse(&reply()).unwrap();
        assert_eq!(parsed.viewer.as_deref(), Some("octocat"));
        assert_eq!(parsed.rate_limit.as_ref().unwrap().remaining, 4998);
        assert_eq!(
            parsed.rate_limit.as_ref().unwrap().reset_at,
            "2026-08-24T23:00:00Z"
        );
        assert!(parsed.warnings.is_empty());

        assert_eq!(parsed.mine.total, 1);
        let mine = &parsed.mine.items[0];
        assert_eq!(mine.id, "PR_mine1");
        assert_eq!(mine.number, 41);
        assert_eq!(mine.title, "Add the thing");
        assert_eq!(mine.url, "https://github.com/acme/app/pull/41");
        assert_eq!(mine.repo, "acme/app");
        assert_eq!(mine.author, "octocat");
        assert!(!mine.is_draft);
        assert_eq!(mine.review_decision.as_deref(), Some("REVIEW_REQUIRED"));
        assert_eq!(mine.ci, CiState::Failure);
        assert_eq!(mine.head_ref, "feat/thing");
        assert_eq!(mine.base_ref, "main");
        assert_eq!(mine.updated_at, "2026-08-24T20:00:00Z");
        assert_eq!(mine.review_request, None);

        assert_eq!(parsed.review_requested.total, 2);
        let [direct, team] = parsed.review_requested.items.as_slice() else {
            panic!("{:?}", parsed.review_requested);
        };
        assert_eq!(direct.review_request, Some(ReviewRequestKind::Direct));
        assert!(direct.is_draft);
        assert_eq!(direct.ci, CiState::Pending);
        assert_eq!(direct.review_decision, None);
        assert_eq!(team.review_request, Some(ReviewRequestKind::Team));
        assert_eq!(team.ci, CiState::None, "a null rollup means no checks");
        assert_eq!(team.author, "", "a deleted author is not an error");

        assert_eq!(parsed.assigned.total, 0);
        assert!(parsed.assigned.items.is_empty());
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
            value["data"]["mine"]["nodes"][0]["commits"]["nodes"][0]["commit"]
                ["statusCheckRollup"] = if state.is_null() {
                json!(null)
            } else {
                json!({ "state": state })
            };
            let parsed = parse(&value).unwrap();
            assert_eq!(parsed.mine.items[0].ci, expected, "{state}");
        }
    }

    #[test]
    fn an_unknown_review_decision_is_dropped_not_fatal() {
        let mut value = reply();
        value["data"]["mine"]["nodes"][0]["reviewDecision"] = json!("BRAND_NEW");
        assert_eq!(parse(&value).unwrap().mine.items[0].review_decision, None);
    }

    #[test]
    fn the_total_is_the_search_count_not_the_page_size() {
        let mut value = reply();
        value["data"]["mine"]["issueCount"] = json!(137);
        let parsed = parse(&value).unwrap();
        assert_eq!(parsed.mine.total, 137);
        assert_eq!(parsed.mine.items.len(), 1);
    }

    #[test]
    fn nodes_without_a_pull_request_shape_are_skipped() {
        let mut value = reply();
        value["data"]["assigned"]["nodes"] = json!([null, {}, { "id": "x", "number": "nope" }]);
        assert!(parse(&value).unwrap().assigned.items.is_empty());
    }

    #[test]
    fn errors_next_to_data_become_warnings() {
        let mut value = reply();
        value["errors"] = json!([{ "message": "Field 'x' is deprecated" }]);
        let parsed = parse(&value).unwrap();
        assert_eq!(parsed.mine.items.len(), 1);
        assert_eq!(parsed.warnings, vec!["Field 'x' is deprecated".to_string()]);
    }

    #[test]
    fn errors_without_data_fail_with_the_first_message() {
        let value =
            json!({ "data": null, "errors": [{ "message": "Bad query" }, { "message": "x" }] });
        assert_eq!(parse(&value).unwrap_err(), "Bad query");
        assert_eq!(parse(&json!({})).unwrap_err(), "reply has no data");
    }

    #[test]
    fn a_missing_rate_limit_or_viewer_is_tolerated() {
        let mut value = reply();
        value["data"]["rateLimit"] = json!(null);
        value["data"]["viewer"] = json!(null);
        let parsed = parse(&value).unwrap();
        assert_eq!(parsed.rate_limit, None);
        assert_eq!(parsed.viewer, None);
    }
}
