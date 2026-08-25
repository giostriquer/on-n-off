//! Canned GraphQL replies shared by the parser and reader tests.

/// One authored PR with failing CI, two review requests (one direct: a draft that already has
/// conflicts, which only `mergeable` reports because the state says `DRAFT`; one via a team), no
/// assignments, and a healthy rate-limit budget.
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
    "assigned": { "issueCount": 0, "nodes": [] }
  }
}"#;
