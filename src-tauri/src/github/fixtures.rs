use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use super::connection::{rate_limited, Budget, Failure, Github};
use crate::cli::AgentCli;
use crate::cli_stub::CliStub;
use crate::dto::GithubStatus;
use crate::http::{head_header, CapturedRequest};

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

pub(super) fn gh(dir: &Path, token: &str) -> AgentCli {
    CliStub::new("gh")
        .log_args("args.txt", true)
        .stdout(token)
        .cli(dir)
}

pub(super) fn gh_runs(dir: &Path) -> usize {
    fs::read_to_string(dir.join("args.txt"))
        .map(|log| log.lines().count())
        .unwrap_or(0)
}

pub(super) fn authorization(request: &CapturedRequest) -> &str {
    head_header(&request.head, "authorization").unwrap_or_default()
}

fn paused_five_minutes(budget: Budget) -> Failure {
    rate_limited(budget, 300, 0)
}

struct Scripted {
    answer: Result<Value, Failure>,
    leaves_reads_paused: bool,
}

#[derive(Default)]
pub(super) struct FakeGithub {
    reads_paused: RefCell<Option<Failure>>,
    events_paused: Option<Failure>,
    replies: RefCell<VecDeque<Scripted>>,
    pages: RefCell<VecDeque<Result<Value, Failure>>>,
    bodies: RefCell<Vec<Value>>,
    pages_asked: RefCell<Vec<(String, u32)>>,
}

impl FakeGithub {
    pub(super) fn paused() -> Self {
        let fake = Self::default();
        *fake.reads_paused.borrow_mut() = Some(paused_five_minutes(Budget::Graphql));
        fake
    }

    pub(super) fn events_paused(self) -> Self {
        Self {
            events_paused: Some(paused_five_minutes(Budget::Rest)),
            ..self
        }
    }

    fn script(self, answer: Result<Value, Failure>, leaves_reads_paused: bool) -> Self {
        self.replies.borrow_mut().push_back(Scripted {
            answer,
            leaves_reads_paused,
        });
        self
    }

    pub(super) fn reply(self, reply: Value) -> Self {
        self.script(Ok(reply), false)
    }

    pub(super) fn reply_leaving_paused(self, reply: Value) -> Self {
        self.script(Ok(reply), true)
    }

    pub(super) fn failing_reply(self, why: &str) -> Self {
        self.failing_with(Failure::new(GithubStatus::Network, why))
    }

    pub(super) fn failing_with(self, failure: Failure) -> Self {
        self.script(Err(failure), false)
    }

    pub(super) fn page(self, events: Value) -> Self {
        self.pages.borrow_mut().push_back(Ok(events));
        self
    }

    pub(super) fn failing_page(self, why: &str) -> Self {
        self.pages
            .borrow_mut()
            .push_back(Err(Failure::new(GithubStatus::Network, why)));
        self
    }

    pub(super) fn body(&self, index: usize) -> Value {
        self.bodies.borrow()[index].clone()
    }

    pub(super) fn requests(&self) -> usize {
        self.bodies.borrow().len()
    }

    pub(super) fn pages_asked(&self) -> Vec<(String, u32)> {
        self.pages_asked.borrow().clone()
    }

    pub(super) fn pages(&self) -> Vec<u32> {
        self.pages_asked
            .borrow()
            .iter()
            .map(|(_, page)| *page)
            .collect()
    }

    pub(super) fn unanswered(&self) -> usize {
        self.replies.borrow().len() + self.pages.borrow().len()
    }
}

impl Github for FakeGithub {
    fn reads_paused(&self) -> Option<Failure> {
        self.reads_paused.borrow().clone()
    }

    fn graphql(&self, body: &Value) -> Result<Value, Failure> {
        self.bodies.borrow_mut().push(body.clone());
        if let Some(pause) = self.reads_paused() {
            return Err(pause);
        }
        let Some(scripted) = self.replies.borrow_mut().pop_front() else {
            return Err(Failure::new(GithubStatus::Network, "no reply scripted"));
        };
        if scripted.leaves_reads_paused {
            *self.reads_paused.borrow_mut() = Some(paused_five_minutes(Budget::Graphql));
        }
        scripted.answer
    }

    fn events(&self, viewer: &str, page: u32) -> Result<Value, Failure> {
        self.pages_asked
            .borrow_mut()
            .push((viewer.to_string(), page));
        if let Some(pause) = &self.events_paused {
            return Err(pause.clone());
        }
        self.pages
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Ok(json!([])))
    }
}
