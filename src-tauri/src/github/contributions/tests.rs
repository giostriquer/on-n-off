use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::*;
use crate::dto::{GithubPrListDto, GithubPrsData};
use crate::paths::scratch_dir;

const NOW: i64 = 1_787_000_000;
const DAY: i64 = 24 * 60 * 60;

#[derive(Default)]
struct FakeGithub {
    pages: RefCell<VecDeque<Result<Value, String>>>,
    replies: RefCell<VecDeque<Result<Value, String>>>,
    pages_asked: RefCell<Vec<u32>>,
    bodies: RefCell<Vec<Value>>,
}

impl FakeGithub {
    fn page(self, events: Value) -> Self {
        self.pages.borrow_mut().push_back(Ok(events));
        self
    }

    fn failing_page(self, why: &str) -> Self {
        self.pages.borrow_mut().push_back(Err(why.to_string()));
        self
    }

    fn reply(self, reply: Value) -> Self {
        self.replies.borrow_mut().push_back(Ok(reply));
        self
    }

    fn failing_reply(self, why: &str) -> Self {
        self.replies.borrow_mut().push_back(Err(why.to_string()));
        self
    }

    fn queries(&self) -> Vec<String> {
        self.bodies
            .borrow()
            .iter()
            .map(|body| body["query"].as_str().unwrap_or_default().to_string())
            .collect()
    }
}

impl Github for FakeGithub {
    fn graphql(&self, body: &Value) -> Result<Value, String> {
        self.bodies.borrow_mut().push(body.clone());
        self.replies
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Err("no reply scripted".into()))
    }

    fn events(&self, page: u32) -> Result<Value, String> {
        self.pages_asked.borrow_mut().push(page);
        self.pages
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Ok(json!([])))
    }
}

fn store_path(prefix: &str) -> PathBuf {
    scratch_dir(prefix).join("contributions.json")
}

fn push(id: u64, repo: &str, git_ref: &str, at: i64) -> Value {
    json!({
        "id": id.to_string(),
        "type": "PushEvent",
        "repo": { "name": repo },
        "payload": { "ref": git_ref, "head": "abc", "before": "def" },
        "created_at": chrono::DateTime::from_timestamp(at, 0).unwrap().to_rfc3339(),
    })
}

fn pr(id: &str, author: &str, head_repo: &str, state: &str) -> Value {
    json!({
        "id": id, "number": 12, "title": "Shared work", "url": format!("https://github.com/{head_repo}/pull/12"),
        "isDraft": false, "updatedAt": "2026-08-24T20:00:00Z", "headRefName": "fix/x", "baseRefName": "main",
        "reviewDecision": null, "repository": { "nameWithOwner": "acme/app", "isInOrganization": true },
        "author": { "login": author }, "mergeable": "MERGEABLE", "mergeStateStatus": "CLEAN",
        "mergeQueueEntry": null, "autoMergeRequest": null,
        "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": "SUCCESS" } } } ] },
        "state": state, "mergedAt": null, "headRefOid": "head1",
        "headRepository": { "nameWithOwner": head_repo },
    })
}

fn lookup(tracked: Value, branches: &[Value]) -> Value {
    let mut data = json!({ "tracked": tracked });
    for (index, nodes) in branches.iter().enumerate() {
        data[format!("b{index}")] = json!({ "pullRequests": { "nodes": nodes } });
    }
    json!({ "data": data })
}

fn commit(parents: u64, authors: &[&str]) -> Value {
    let authors: Vec<Value> = authors
        .iter()
        .map(|login| json!({ "user": { "login": login } }))
        .collect();
    json!({ "commit": { "parents": { "totalCount": parents }, "authors": { "nodes": authors } } })
}

fn commits(per_pr: &[Vec<Value>]) -> Value {
    let nodes: Vec<Value> = per_pr
        .iter()
        .map(|commits| json!({ "commits": { "nodes": commits } }))
        .collect();
    json!({ "data": { "nodes": nodes } })
}

fn one_push() -> FakeGithub {
    FakeGithub::default().page(json!([push(10, "acme/app", "refs/heads/fix/x", NOW - 60)]))
}

fn read_now(github: &FakeGithub, path: &std::path::Path) -> Contributed {
    read(github, path, "octocat", &[], NOW)
}

#[test]
fn a_pr_someone_else_opened_on_a_branch_you_pushed_joins_once_it_carries_a_commit_you_wrote() {
    let path = store_path("contrib-found");
    let github = one_push()
        .reply(lookup(
            json!([]),
            &[json!([pr("PR_alice", "alice", "acme/app", "OPEN")])],
        ))
        .reply(commits(&[vec![
            commit(1, &["alice"]),
            commit(1, &["octocat"]),
        ]]));

    let found = read_now(&github, &path);

    assert!(found.complete);
    assert_eq!(found.warnings, Vec::<String>::new());
    assert_eq!(found.open.len(), 1);
    assert_eq!(found.open[0].id, "PR_alice");
    assert_eq!(found.open[0].author, "alice");
    assert!(found.open[0].contributed);
    assert!(found.merged.is_empty());
    assert_eq!(*github.pages_asked.borrow(), vec![1]);
}

#[test]
fn a_merge_commit_of_yours_is_not_a_commit_you_wrote() {
    let path = store_path("contrib-merge-commit");
    let github = one_push()
        .reply(lookup(
            json!([]),
            &[json!([pr("PR_alice", "alice", "acme/app", "OPEN")])],
        ))
        .reply(commits(&[vec![
            commit(1, &["alice"]),
            commit(2, &["octocat"]),
        ]]));

    let found = read_now(&github, &path);

    assert!(found.complete);
    assert!(found.open.is_empty());
}

#[test]
fn a_commit_you_co_authored_counts() {
    let path = store_path("contrib-co-author");
    let github = one_push()
        .reply(lookup(
            json!([]),
            &[json!([pr("PR_alice", "alice", "acme/app", "OPEN")])],
        ))
        .reply(commits(&[vec![commit(1, &["alice", "OctoCat"])]]));

    assert_eq!(read_now(&github, &path).open.len(), 1);
}

#[test]
fn a_pr_you_opened_is_left_to_the_mine_search() {
    let path = store_path("contrib-own");
    let github = one_push().reply(lookup(
        json!([]),
        &[json!([pr("PR_own", "octocat", "acme/app", "OPEN")])],
    ));

    let found = read_now(&github, &path);

    assert!(found.open.is_empty());
    assert_eq!(github.bodies.borrow().len(), 1, "nothing to verify");
}

#[test]
fn a_fork_pr_with_the_same_branch_name_is_not_on_the_branch_you_pushed() {
    let path = store_path("contrib-fork");
    let github = one_push().reply(lookup(
        json!([]),
        &[json!([pr("PR_fork", "alice", "alice/app", "OPEN")])],
    ));

    let found = read_now(&github, &path);

    assert!(found.open.is_empty());
    assert_eq!(github.bodies.borrow().len(), 1, "nothing to verify");
}

#[test]
fn only_branch_pushes_are_looked_up() {
    let path = store_path("contrib-tags");
    let github = FakeGithub::default().page(json!([
        push(12, "acme/app", "refs/tags/v1.0.0", NOW - 60),
        { "id": "11", "type": "IssuesEvent", "repo": { "name": "acme/app" }, "payload": {}, "created_at": "2026-08-24T20:00:00Z" },
    ]));

    let found = read_now(&github, &path);

    assert!(found.complete);
    assert!(github.bodies.borrow().is_empty(), "no branch, no lookup");
}

#[test]
fn every_repository_and_branch_name_travels_as_a_variable() {
    let path = store_path("contrib-variables");
    let github = FakeGithub::default()
        .page(json!([push(
            10,
            "acme/app",
            "refs/heads/fix/\"quoted\" { x }",
            NOW - 60
        )]))
        .reply(lookup(json!([]), &[json!([])]));

    read_now(&github, &path);

    let body = github.bodies.borrow()[0].clone();
    let query = body["query"].as_str().unwrap();
    assert!(!query.contains("quoted"), "{query}");
    assert!(!query.contains("acme"), "{query}");
    assert_eq!(body["variables"]["o0"], "acme");
    assert_eq!(body["variables"]["n0"], "app");
    assert_eq!(body["variables"]["h0"], "fix/\"quoted\" { x }");
    assert_eq!(body["variables"]["ids"], json!([]));
    assert!(query.contains("headRefName: $h0, states: OPEN"));
}

#[test]
fn events_are_read_at_most_once_a_minute_and_only_back_to_the_newest_one_read() {
    let path = store_path("contrib-events-window");
    let first = FakeGithub::default()
        .page(json!([push(10, "acme/app", "refs/heads/fix/x", NOW - 60)]))
        .reply(lookup(json!([]), &[json!([])]));
    read_now(&first, &path);

    let soon = FakeGithub::default().reply(lookup(json!([]), &[json!([])]));
    read(&soon, &path, "octocat", &[], NOW + 59);
    assert!(soon.pages_asked.borrow().is_empty());

    let later = FakeGithub::default()
        .page(json!([
            push(12, "acme/app", "refs/heads/fix/y", NOW + 30),
            push(10, "acme/app", "refs/heads/fix/x", NOW - 60),
            push(9, "acme/app", "refs/heads/fix/old", NOW - 90),
        ]))
        .reply(lookup(json!([]), &[json!([]), json!([])]));
    read(&later, &path, "octocat", &[], NOW + 60);

    let query = later.queries()[0].clone();
    assert!(query.contains("b1:"), "{query}");
    assert!(
        !query.contains("b2:"),
        "event 9 is older than the newest one read: {query}"
    );
    let body = later.bodies.borrow()[0].clone();
    assert_eq!(body["variables"]["h0"], "fix/y");
    assert_eq!(body["variables"]["h1"], "fix/x");
}

#[test]
fn a_full_page_of_events_reads_the_next_up_to_three() {
    let path = store_path("contrib-pages");
    let full = |from: u64| -> Value {
        Value::Array(
            (0..100)
                .map(|offset| push(from - offset, "acme/app", "refs/heads/fix/x", NOW - 60))
                .collect(),
        )
    };
    let github = FakeGithub::default()
        .page(full(1_000))
        .page(full(900))
        .page(full(800))
        .page(full(700))
        .reply(lookup(json!([]), &[json!([])]));

    read_now(&github, &path);

    assert_eq!(*github.pages_asked.borrow(), vec![1, 2, 3]);
}

#[test]
fn a_verified_pr_is_never_checked_again_and_an_unverified_one_only_when_its_head_moves() {
    let path = store_path("contrib-verify-once");
    let first = one_push()
        .reply(lookup(
            json!([]),
            &[json!([
                pr("PR_yes", "alice", "acme/app", "OPEN"),
                pr("PR_no", "bob", "acme/app", "OPEN"),
            ])],
        ))
        .reply(commits(&[
            vec![commit(1, &["octocat"])],
            vec![commit(1, &["bob"])],
        ]));
    assert_eq!(read_now(&first, &path).open.len(), 1);

    let mut moved = pr("PR_no", "bob", "acme/app", "OPEN");
    moved["headRefOid"] = json!("head2");
    let unchanged = || {
        lookup(
            json!([
                pr("PR_yes", "alice", "acme/app", "OPEN"),
                pr("PR_no", "bob", "acme/app", "OPEN")
            ]),
            &[json!([
                pr("PR_yes", "alice", "acme/app", "OPEN"),
                pr("PR_no", "bob", "acme/app", "OPEN")
            ])],
        )
    };
    let quiet = FakeGithub::default().reply(unchanged());
    assert_eq!(read(&quiet, &path, "octocat", &[], NOW + 30).open.len(), 1);
    assert_eq!(
        quiet.bodies.borrow().len(),
        1,
        "no head moved, nothing to verify"
    );

    let pushed = FakeGithub::default()
        .reply(lookup(
            json!([pr("PR_yes", "alice", "acme/app", "OPEN"), moved.clone()]),
            &[json!([pr("PR_yes", "alice", "acme/app", "OPEN"), moved])],
        ))
        .reply(commits(&[vec![
            commit(1, &["bob"]),
            commit(1, &["octocat"]),
        ]]));
    let found = read(&pushed, &path, "octocat", &[], NOW + 40);

    assert_eq!(found.open.len(), 2);
    assert_eq!(
        pushed.bodies.borrow()[1]["variables"]["ids"],
        json!(["PR_no"])
    );
}

#[test]
fn a_merged_contribution_moves_to_the_merged_list_and_a_closed_one_is_forgotten() {
    let path = store_path("contrib-merged");
    let first = one_push()
        .reply(lookup(
            json!([]),
            &[json!([
                pr("PR_merged", "alice", "acme/app", "OPEN"),
                pr("PR_closed", "bob", "acme/app", "OPEN"),
            ])],
        ))
        .reply(commits(&[
            vec![commit(1, &["octocat"])],
            vec![commit(1, &["octocat"])],
        ]));
    assert_eq!(read_now(&first, &path).open.len(), 2);

    let mut merged = pr("PR_merged", "alice", "acme/app", "MERGED");
    merged["mergedAt"] = json!("2026-08-24T21:00:00Z");
    let after = FakeGithub::default().reply(lookup(
        json!([merged, pr("PR_closed", "bob", "acme/app", "CLOSED")]),
        &[json!([])],
    ));
    let found = read(&after, &path, "octocat", &[], NOW + 30);

    assert!(found.open.is_empty());
    assert_eq!(found.merged.len(), 1);
    assert_eq!(found.merged[0].id, "PR_merged");
    assert!(found.merged[0].contributed);
    let ids = after.bodies.borrow()[0]["variables"]["ids"].clone();
    assert_eq!(ids, json!(["PR_merged", "PR_closed"]));

    let mut still_merged = pr("PR_merged", "alice", "acme/app", "MERGED");
    still_merged["mergedAt"] = json!("2026-08-24T21:00:00Z");
    let next = FakeGithub::default().reply(lookup(json!([still_merged]), &[json!([])]));
    read(&next, &path, "octocat", &[], NOW + 60);
    assert_eq!(
        next.bodies.borrow()[0]["variables"]["ids"],
        json!(["PR_merged"]),
        "the closed one is no longer tracked"
    );
}

#[test]
fn a_merged_contribution_is_kept_for_fourteen_days_and_a_pushed_branch_for_thirty() {
    let path = store_path("contrib-expiry");
    let first = one_push()
        .reply(lookup(
            json!([]),
            &[json!([pr("PR_merged", "alice", "acme/app", "OPEN")])],
        ))
        .reply(commits(&[vec![commit(1, &["octocat"])]]));
    read_now(&first, &path);
    let mut merged = pr("PR_merged", "alice", "acme/app", "MERGED");
    merged["mergedAt"] = json!(chrono::DateTime::from_timestamp(NOW, 0)
        .unwrap()
        .to_rfc3339());
    let merging = FakeGithub::default().reply(lookup(json!([merged.clone()]), &[json!([])]));
    read(&merging, &path, "octocat", &[], NOW + 30);

    let day13 = FakeGithub::default().reply(lookup(json!([merged]), &[json!([])]));
    read(&day13, &path, "octocat", &[], NOW + 13 * DAY);
    assert_eq!(
        day13.bodies.borrow()[0]["variables"]["ids"],
        json!(["PR_merged"])
    );

    let day14 = FakeGithub::default().reply(lookup(json!([]), &[json!([])]));
    read(&day14, &path, "octocat", &[], NOW + 14 * DAY);
    assert_eq!(day14.bodies.borrow()[0]["variables"]["ids"], json!([]));
    assert_eq!(day14.bodies.borrow()[0]["variables"]["h0"], "fix/x");

    let day30 = FakeGithub::default();
    let found = read(&day30, &path, "octocat", &[], NOW + 30 * DAY);
    assert!(found.complete);
    assert!(day30.bodies.borrow().is_empty(), "nothing left to look up");
}

#[test]
fn a_failed_lookup_is_incomplete_and_says_why() {
    let path = store_path("contrib-lookup-failed");
    let github = one_push().failing_reply("HTTP 502");

    let found = read_now(&github, &path);

    assert!(!found.complete);
    assert_eq!(
        found.warnings,
        vec!["Could not look up the pull requests on your pushed branches (HTTP 502).".to_string()]
    );
}

#[test]
fn a_failed_events_read_still_looks_up_the_branches_already_remembered() {
    let path = store_path("contrib-events-failed");
    let first = one_push().reply(lookup(json!([]), &[json!([])]));
    read_now(&first, &path);

    let github = FakeGithub::default()
        .failing_page("HTTP 503")
        .reply(lookup(json!([]), &[json!([])]));
    let found = read(&github, &path, "octocat", &[], NOW + 60);

    assert!(found.complete);
    assert_eq!(
        found.warnings,
        vec!["Could not read your recent pushes (HTTP 503).".to_string()]
    );
    assert_eq!(github.bodies.borrow()[0]["variables"]["h0"], "fix/x");

    let retry = FakeGithub::default().reply(lookup(json!([]), &[json!([])]));
    read(&retry, &path, "octocat", &[], NOW + 61);
    assert_eq!(
        *retry.pages_asked.borrow(),
        vec![1],
        "a failed read is retried at once"
    );
}

#[test]
fn another_github_account_starts_from_nothing() {
    let path = store_path("contrib-viewer");
    let first = one_push().reply(lookup(json!([]), &[json!([])]));
    read_now(&first, &path);

    let other = FakeGithub::default();
    let found = read(&other, &path, "hubot", &[], NOW + 1);

    assert!(found.complete);
    assert_eq!(*other.pages_asked.borrow(), vec![1]);
    assert!(
        other.bodies.borrow().is_empty(),
        "the other account's branches are gone"
    );
}

#[test]
fn an_unreadable_store_starts_from_nothing() {
    let path = store_path("contrib-unreadable");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{ not json").unwrap();
    let github = one_push().reply(lookup(json!([]), &[json!([])]));

    assert!(read_now(&github, &path).complete);
    assert_eq!(github.bodies.borrow().len(), 1);
}

#[test]
fn a_contribution_outside_the_scope_is_not_shown() {
    let path = store_path("contrib-scope");
    let github = one_push()
        .reply(lookup(
            json!([]),
            &[json!([pr("PR_alice", "alice", "acme/app", "OPEN")])],
        ))
        .reply(commits(&[vec![commit(1, &["octocat"])]]));

    let found = read(&github, &path, "octocat", &["org:other".to_string()], NOW);

    assert!(found.complete);
    assert!(found.open.is_empty());
}

#[test]
fn the_scope_matches_an_owner_or_one_repository_like_the_search_does() {
    let repo = "Acme/App";
    assert!(in_scope(repo, &[]));
    assert!(in_scope(repo, &["org:acme".into()]));
    assert!(in_scope(repo, &["user:ACME".into()]));
    assert!(in_scope(repo, &["repo:acme/app".into()]));
    assert!(in_scope(
        repo,
        &["org:other".into(), "repo:acme/app".into()]
    ));
    assert!(!in_scope(repo, &["repo:acme/application".into()]));
    assert!(!in_scope(repo, &["org:acm".into()]));
}

#[test]
fn contributions_join_the_lists_once_and_raise_their_totals() {
    let contributed = |id: &str| {
        let mut found =
            crate::github::parse::pull_request(&pr(id, "alice", "acme/app", "OPEN")).unwrap();
        found.contributed = true;
        found
    };
    let mut data = GithubPrsData {
        mine: GithubPrListDto {
            total: 7,
            items: vec![contributed("PR_a")],
        },
        merged: GithubPrListDto {
            total: 3,
            items: Vec::new(),
        },
        ..GithubPrsData::default()
    };

    add_to(
        &mut data,
        vec![contributed("PR_a"), contributed("PR_b")],
        vec![contributed("PR_c")],
    );

    let ids = |list: &GithubPrListDto| {
        list.items
            .iter()
            .map(|pr| pr.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&data.mine), vec!["PR_a", "PR_b"]);
    assert_eq!(data.mine.total, 8);
    assert_eq!(ids(&data.merged), vec!["PR_c"]);
    assert_eq!(data.merged.total, 4);
}
