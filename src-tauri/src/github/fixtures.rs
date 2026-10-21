use serde_json::{json, Value};

pub(super) const REPLY: &str = r#"{
  "data": {
    "viewer": { "login": "octocat" },
    "rateLimit": { "remaining": 4998, "resetAt": "2026-08-24T23:00:00Z" },
    "mine": {
      "issueCount": 1,
      "nodes": [
        {
          "id": "PR_mine1", "number": 41, "title": "Add the thing", "url": "https://github.com/acme/app/pull/41",
          "isDraft": false, "updatedAt": "2026-08-24T20:00:00Z", "headRefName": "feat/thing", "baseRefName": "main",
          "reviewDecision": "REVIEW_REQUIRED", "repository": { "nameWithOwner": "acme/app" }, "author": { "login": "octocat" },
          "mergeable": "MERGEABLE", "mergeStateStatus": "BLOCKED", "mergeQueueEntry": null, "autoMergeRequest": null,
          "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": "FAILURE" } } } ] }
        }
      ]
    },
    "review": {
      "issueCount": 2,
      "nodes": [
        {
          "id": "PR_rev1", "number": 7, "title": "Direct ask", "url": "https://github.com/acme/lib/pull/7",
          "isDraft": true, "updatedAt": "2026-08-24T19:00:00Z", "headRefName": "fix/x", "baseRefName": "main",
          "reviewDecision": null, "repository": { "nameWithOwner": "acme/lib" }, "author": { "login": "alice" },
          "mergeable": "CONFLICTING", "mergeStateStatus": "DRAFT", "mergeQueueEntry": null, "autoMergeRequest": null,
          "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": "PENDING" } } } ] }
        },
        {
          "id": "PR_rev2", "number": 8, "title": "Team ask", "url": "https://github.com/acme/lib/pull/8",
          "isDraft": false, "updatedAt": "2026-08-24T18:00:00Z", "headRefName": "fix/y", "baseRefName": "main",
          "reviewDecision": "APPROVED", "repository": { "nameWithOwner": "acme/lib" }, "author": null,
          "mergeable": "CONFLICTING", "mergeStateStatus": "DIRTY", "mergeQueueEntry": null,
          "autoMergeRequest": { "enabledAt": "2026-08-24T17:00:00Z" },
          "commits": { "nodes": [ { "commit": { "statusCheckRollup": null } } ] }
        }
      ]
    },
    "direct": { "issueCount": 1, "nodes": [ { "id": "PR_rev1" } ] },
    "assigned": { "issueCount": 0, "nodes": [] },
    "merged": {
      "issueCount": 3,
      "nodes": [
        {
          "id": "PR_merged1", "number": 39, "title": "Land the other thing", "url": "https://github.com/acme/app/pull/39",
          "isDraft": false, "updatedAt": "2026-08-24T19:30:00Z", "headRefName": "feat/other", "baseRefName": "main",
          "reviewDecision": "APPROVED", "repository": { "nameWithOwner": "acme/app" }, "author": { "login": "octocat" },
          "mergeable": "UNKNOWN", "mergeStateStatus": "UNKNOWN", "mergeQueueEntry": null, "autoMergeRequest": null,
          "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": "SUCCESS" } } } ] }
        }
      ]
    }
  }
}"#;

pub(super) fn contribution_node(id: &str, author: &str, head_repo: &str, state: &str) -> Value {
    json!({
        "id": id, "number": 12, "title": "Shared work", "url": "https://github.com/acme/app/pull/12",
        "isDraft": false, "updatedAt": "2026-08-24T20:00:00Z", "headRefName": "fix/x", "baseRefName": "main",
        "reviewDecision": null, "repository": { "nameWithOwner": "acme/app", "isInOrganization": true },
        "author": { "login": author }, "mergeable": "MERGEABLE", "mergeStateStatus": "CLEAN",
        "mergeQueueEntry": null, "autoMergeRequest": null,
        "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": "SUCCESS" } } } ] },
        "state": state, "mergedAt": null, "headRefOid": "head1",
        "headRepository": { "nameWithOwner": head_repo },
    })
}

pub(super) fn lookup_reply(tracked: Value, branches: &[Value]) -> Value {
    let mut data = json!({ "tracked": tracked });
    for (index, nodes) in branches.iter().enumerate() {
        data[format!("b{index}")] = json!({
            "defaultBranchRef": { "name": "main" },
            "pullRequests": { "nodes": nodes },
        });
    }
    json!({ "data": data })
}

pub(super) fn commit(parents: u64, authors: &[&str]) -> Value {
    let authors: Vec<Value> = authors
        .iter()
        .map(|login| json!({ "user": { "login": login } }))
        .collect();
    json!({ "commit": { "parents": { "totalCount": parents }, "authors": { "nodes": authors } } })
}

pub(super) fn commits_reply(per_pr: &[Vec<Value>]) -> Value {
    let nodes: Vec<Value> = per_pr
        .iter()
        .map(|commits| json!({ "commits": { "nodes": commits } }))
        .collect();
    json!({ "data": { "nodes": nodes } })
}
