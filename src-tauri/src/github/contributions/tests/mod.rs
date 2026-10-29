mod discovery;
mod tracking;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::*;
use crate::github::fixtures::{commit, commits_reply, contribution_node, lookup_reply};
use crate::paths::scratch_dir;

const NOW: i64 = 1_787_000_000;
const DAY: i64 = 24 * 60 * 60;

#[derive(Default)]
struct FakeGithub {
    pages: RefCell<VecDeque<Result<Value, String>>>,
    replies: RefCell<VecDeque<Result<Value, String>>>,
    pages_asked: RefCell<Vec<(String, u32)>>,
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

    fn body(&self, index: usize) -> Value {
        self.bodies.borrow()[index].clone()
    }

    fn pages(&self) -> Vec<u32> {
        self.pages_asked
            .borrow()
            .iter()
            .map(|(_, page)| *page)
            .collect()
    }

    fn requests(&self) -> usize {
        self.bodies.borrow().len()
    }

    fn branches_looked_up(&self, index: usize) -> Vec<String> {
        let variables = self.body(index)["variables"].clone();
        (0..)
            .map_while(|n| {
                let branch = variables[format!("h{n}")].as_str()?;
                Some(format!(
                    "{}/{}:{branch}",
                    variables[format!("o{n}")].as_str().unwrap_or_default(),
                    variables[format!("n{n}")].as_str().unwrap_or_default()
                ))
            })
            .collect()
    }

    fn ids_sent(&self, index: usize) -> Value {
        self.body(index)["variables"]["ids"].clone()
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

    fn events(&self, viewer: &str, page: u32) -> Result<Value, String> {
        self.pages_asked
            .borrow_mut()
            .push((viewer.to_string(), page));
        self.pages
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Ok(json!([])))
    }
}

fn store_path(prefix: &str) -> PathBuf {
    scratch_dir(prefix).join("contributions.json")
}

fn at(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .unwrap()
        .to_rfc3339()
}

fn push(id: u64, repo: &str, git_ref: &str, pushed_at: i64) -> Value {
    json!({
        "id": id.to_string(),
        "type": "PushEvent",
        "repo": { "name": repo },
        "payload": { "ref": git_ref, "head": "abc", "before": "def" },
        "created_at": at(pushed_at),
    })
}

fn pr(id: &str, author: &str) -> Value {
    contribution_node(id, author, "acme/app", "OPEN")
}

fn merged_pr(id: &str, merged_at: i64) -> Value {
    let mut node = contribution_node(id, "alice", "acme/app", "MERGED");
    node["mergedAt"] = json!(at(merged_at));
    node
}

fn moved(mut node: Value, head: &str) -> Value {
    node["headRefOid"] = json!(head);
    node
}

fn yours() -> Vec<Value> {
    vec![commit(1, &["octocat"])]
}

fn theirs() -> Vec<Value> {
    vec![commit(1, &["bob"])]
}

fn one_push() -> FakeGithub {
    FakeGithub::default().page(json!([push(10, "acme/app", "refs/heads/fix/x", NOW - 60)]))
}

fn empty_lookup() -> Value {
    lookup_reply(json!([]), &[json!([])])
}

fn read_at(github: &FakeGithub, path: &Path, now_secs: i64) -> Contributed {
    read(
        Some(github as &dyn Github),
        path,
        "octocat",
        &[],
        &[],
        now_secs,
    )
}

fn ids(prs: &[GithubPrDto]) -> Vec<&str> {
    prs.iter().map(|pr| pr.id.as_str()).collect()
}

fn found_alice(path: &Path) {
    let github = one_push()
        .reply(lookup_reply(json!([]), &[json!([pr("PR_alice", "alice")])]))
        .reply(commits_reply(&[yours()]));
    assert_eq!(ids(&read_at(&github, path, NOW).open), vec!["PR_alice"]);
}

#[test]
fn contributions_join_the_lists_once_and_raise_their_totals() {
    let contributed = |id: &str| {
        let mut found = crate::github::parse::pull_request(&pr(id, "alice")).unwrap();
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

    assert_eq!(ids(&data.mine.items), vec!["PR_a", "PR_b"]);
    assert_eq!(data.mine.total, 8);
    assert_eq!(ids(&data.merged.items), vec!["PR_c"]);
    assert_eq!(data.merged.total, 4);
}
