use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::merge;
use super::parse::pull_request;
use super::query::{in_scope, PR_FRAGMENT};
use crate::dto::{GithubPrDto, GithubPrListDto, GithubPrsData};
use crate::usage::cache_io::atomic_write;

const SCHEMA_VERSION: u8 = 1;
const EVENTS_EVERY_SECS: i64 = 60;
const EVENT_PAGES: u32 = 3;
pub(super) const EVENTS_PER_PAGE: usize = 100;
const BRANCH_KEEP_SECS: i64 = 30 * 24 * 60 * 60;
const MERGED_KEEP_SECS: i64 = 14 * 24 * 60 * 60;
const MAX_BRANCHES: usize = 50;
const PRS_PER_BRANCH: u32 = 5;
const VERIFY_PER_READ: usize = 10;

const CONTRIBUTION_FRAGMENT: &str = "\
fragment contribution on PullRequest {
  ...pr state mergedAt headRefOid headRepository { nameWithOwner }
}
";

const VERIFY: &str = "\
query($ids: [ID!]!) {
  nodes(ids: $ids) {
    ... on PullRequest {
      commits(last: 100) {
        nodes { commit { parents { totalCount } authors(first: 10) { nodes { user { login } } } } }
      }
    }
  }
}";

pub(super) trait Github {
    fn graphql(&self, body: &Value) -> Result<Value, String>;
    fn events(&self, viewer: &str, page: u32) -> Result<Value, String>;
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Contributed {
    pub(super) open: Vec<GithubPrDto>,
    pub(super) merged: Vec<GithubPrDto>,
    pub(super) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    schema_version: u8,
    viewer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    newest_event: Option<u64>,
    #[serde(default)]
    events_read_at: i64,
    #[serde(default)]
    branches: Vec<Branch>,
    #[serde(default)]
    tracked: Vec<Tracked>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dropped: Vec<Branch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Branch {
    repo: String,
    name: String,
    pushed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tracked {
    id: String,
    repo: String,
    branch: String,
    #[serde(default)]
    verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    checked_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    merged_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last: Option<GithubPrDto>,
}

struct Pushes {
    branches: Vec<Branch>,
    newest: Option<u64>,
}

struct Found {
    pr: GithubPrDto,
    head: Option<String>,
    merged_at: Option<i64>,
}

#[derive(Default)]
struct Lookup {
    found: HashMap<String, Found>,
    gone: HashSet<String>,
    lost: HashSet<(String, String)>,
    discovered: Vec<Tracked>,
}

impl Branch {
    fn key(&self) -> (String, String) {
        (self.repo.clone(), self.name.clone())
    }

    fn is(&self, other: &Branch) -> bool {
        self.repo == other.repo && self.name == other.name
    }
}

fn keep_latest(branches: &mut Vec<Branch>, push: Branch) {
    match branches.iter_mut().find(|known| known.is(&push)) {
        Some(known) => known.pushed_at = known.pushed_at.max(push.pushed_at),
        None => branches.push(push),
    }
}

impl Store {
    fn new(viewer: &str) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            viewer: viewer.to_string(),
            newest_event: None,
            events_read_at: 0,
            branches: Vec::new(),
            tracked: Vec::new(),
            dropped: Vec::new(),
        }
    }

    fn remember(&mut self, pushes: Pushes, now_secs: i64) {
        for push in pushes.branches {
            let seen_dropped = self
                .dropped
                .iter()
                .any(|dropped| dropped.is(&push) && dropped.pushed_at >= push.pushed_at);
            if !seen_dropped {
                keep_latest(&mut self.branches, push);
            }
        }
        self.newest_event = self.newest_event.max(pushes.newest);
        self.events_read_at = now_secs;
    }

    fn forget_old(&mut self, now_secs: i64) {
        self.branches
            .retain(|branch| now_secs - branch.pushed_at < BRANCH_KEEP_SECS);
        self.branches
            .sort_by_key(|branch| std::cmp::Reverse(branch.pushed_at));
        self.branches.truncate(MAX_BRANCHES);
        self.dropped
            .retain(|branch| now_secs - branch.pushed_at < BRANCH_KEEP_SECS);
        let branches = &self.branches;
        self.tracked.retain(|tracked| {
            let fresh = tracked
                .merged_at
                .is_none_or(|at| now_secs - at < MERGED_KEEP_SECS);
            let followed = tracked.verified
                || branches
                    .iter()
                    .any(|branch| branch.repo == tracked.repo && branch.name == tracked.branch);
            fresh && followed
        });
    }

    fn reconcile(&mut self, lookup: Lookup) {
        let (lost, kept): (Vec<Branch>, Vec<Branch>) = std::mem::take(&mut self.branches)
            .into_iter()
            .partition(|branch| lookup.lost.contains(&branch.key()));
        self.branches = kept;
        for branch in lost {
            keep_latest(&mut self.dropped, branch);
        }
        self.tracked
            .retain(|tracked| !lookup.gone.contains(&tracked.id));
        self.tracked.extend(lookup.discovered);
        for tracked in &mut self.tracked {
            if let Some(found) = lookup.found.get(&tracked.id) {
                tracked.head.clone_from(&found.head);
                tracked.merged_at = found.merged_at.map(|at| tracked.merged_at.unwrap_or(at));
                tracked.last = Some(found.pr.clone());
            }
        }
    }

    fn shown(&self, scopes: &[String]) -> (Vec<GithubPrDto>, Vec<GithubPrDto>) {
        let mut open = Vec::new();
        let mut merged = Vec::new();
        for tracked in self.tracked.iter().filter(|tracked| tracked.verified) {
            let Some(pr) = tracked.last.clone().filter(|pr| in_scope(&pr.repo, scopes)) else {
                continue;
            };
            if tracked.merged_at.is_some() {
                merged.push(pr);
            } else {
                open.push(pr);
            }
        }
        (open, merged)
    }
}

pub(super) fn read(
    github: Option<&dyn Github>,
    path: &Path,
    viewer: &str,
    scopes: &[String],
    now_secs: i64,
) -> Contributed {
    let mut store = load(path, viewer);
    let before = store.clone();
    let mut warnings = Vec::new();
    match github {
        Some(github) => refresh(github, &mut store, viewer, now_secs, &mut warnings),
        None => store.forget_old(now_secs),
    }
    if store != before {
        if let Err(why) = save(path, &store) {
            warnings.push(format!("Your pushed branches were not remembered. {why}"));
        }
    }
    let (open, merged) = store.shown(scopes);
    Contributed {
        open,
        merged,
        warnings,
    }
}

pub(super) fn add_to(data: &mut GithubPrsData, open: Vec<GithubPrDto>, merged: Vec<GithubPrDto>) {
    append(&mut data.mine, open);
    append(&mut data.merged, merged);
}

fn append(list: &mut GithubPrListDto, prs: Vec<GithubPrDto>) {
    for pr in prs {
        if list.items.iter().all(|known| known.id != pr.id) {
            list.items.push(pr);
            list.total += 1;
        }
    }
}

fn refresh(
    github: &dyn Github,
    store: &mut Store,
    viewer: &str,
    now_secs: i64,
    warnings: &mut Vec<String>,
) {
    if now_secs - store.events_read_at >= EVENTS_EVERY_SECS {
        match read_pushes(github, viewer, store.newest_event) {
            Ok(pushes) => store.remember(pushes, now_secs),
            Err(why) => warnings.push(format!("Your recent pushes were not read. {why}")),
        }
    }
    store.forget_old(now_secs);
    if store.branches.is_empty() && store.tracked.is_empty() {
        return;
    }
    let ids: Vec<String> = store
        .tracked
        .iter()
        .map(|tracked| tracked.id.clone())
        .collect();
    let lookup = github
        .graphql(&lookup_body(&store.branches, &ids))
        .and_then(|reply| parse_lookup(&reply, &store.branches, &ids, viewer, now_secs));
    match lookup {
        Ok(lookup) => {
            store.reconcile(lookup);
            if let Err(why) = verify(github, store, viewer) {
                warnings.push(format!(
                    "Your commits on pull requests were not checked. {why}"
                ));
            }
        }
        Err(why) => warnings.push(format!(
            "Pull requests on your pushed branches were not looked up. {why}"
        )),
    }
}

fn read_pushes(github: &dyn Github, viewer: &str, since: Option<u64>) -> Result<Pushes, String> {
    let mut pushes = Pushes {
        branches: Vec::new(),
        newest: None,
    };
    for page in 1..=EVENT_PAGES {
        let reply = github.events(viewer, page)?;
        let events = reply
            .as_array()
            .ok_or_else(|| "GitHub sent an unexpected reply.".to_string())?;
        let mut caught_up = events.len() < EVENTS_PER_PAGE;
        for event in events {
            let Some(id) = event["id"].as_str().and_then(|id| id.parse::<u64>().ok()) else {
                continue;
            };
            caught_up |= since.is_some_and(|since| id <= since);
            pushes.newest = pushes.newest.max(Some(id));
            pushes.branches.extend(pushed_branch(event));
        }
        if caught_up {
            break;
        }
    }
    Ok(pushes)
}

fn pushed_branch(event: &Value) -> Option<Branch> {
    if event["type"] != "PushEvent" {
        return None;
    }
    let repo = event["repo"]["name"].as_str().filter(|repo| {
        repo.split_once('/')
            .is_some_and(|(owner, name)| !owner.is_empty() && !name.is_empty())
    })?;
    let name = event["payload"]["ref"]
        .as_str()?
        .strip_prefix("refs/heads/")?;
    Some(Branch {
        repo: repo.to_string(),
        name: name.to_string(),
        pushed_at: timestamp(&event["created_at"])?,
    })
}

fn lookup_body(branches: &[Branch], ids: &[String]) -> Value {
    let mut parameters = vec!["$ids: [ID!]!".to_string()];
    let mut fields = vec!["  tracked: nodes(ids: $ids) { ...contribution }".to_string()];
    let mut variables = Map::new();
    variables.insert("ids".into(), json!(ids));
    for (index, branch) in branches.iter().enumerate() {
        let Some((owner, name)) = branch.repo.split_once('/') else {
            continue;
        };
        parameters.push(format!(
            "$o{index}: String!, $n{index}: String!, $h{index}: String!"
        ));
        fields.push(format!(
            "  b{index}: repository(owner: $o{index}, name: $n{index}) {{ defaultBranchRef {{ name }} \
             pullRequests(headRefName: $h{index}, states: OPEN, first: {PRS_PER_BRANCH}) \
             {{ nodes {{ ...contribution }} }} }}"
        ));
        variables.insert(format!("o{index}"), json!(owner));
        variables.insert(format!("n{index}"), json!(name));
        variables.insert(format!("h{index}"), json!(branch.name));
    }
    json!({
        "query": format!(
            "query({}) {{\n{}\n}}\n{CONTRIBUTION_FRAGMENT}{PR_FRAGMENT}",
            parameters.join(", "),
            fields.join("\n")
        ),
        "variables": variables,
    })
}

fn parse_lookup(
    reply: &Value,
    branches: &[Branch],
    ids: &[String],
    viewer: &str,
    now_secs: i64,
) -> Result<Lookup, String> {
    let data = &reply["data"];
    let tracked = data["tracked"]
        .as_array()
        .filter(|nodes| nodes.len() == ids.len())
        .ok_or_else(|| first_error(reply))?;
    let mut lookup = Lookup::default();
    for (id, node) in ids.iter().zip(tracked) {
        match node["state"].as_str() {
            Some("OPEN" | "MERGED") => {
                if let Some(found) = found(node, now_secs) {
                    lookup.found.insert(id.clone(), found);
                }
            }
            _ => {
                lookup.gone.insert(id.clone());
            }
        }
    }
    for (index, branch) in branches.iter().enumerate() {
        let repository = &data[format!("b{index}")];
        let default = repository["defaultBranchRef"]["name"].as_str();
        if repository.is_null() || default == Some(branch.name.as_str()) {
            lookup.lost.insert(branch.key());
            continue;
        }
        for node in repository["pullRequests"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if !names(&node["headRepository"]["nameWithOwner"], &branch.repo)
                || names(&node["author"]["login"], viewer)
            {
                continue;
            }
            let Some(found) = found(node, now_secs) else {
                continue;
            };
            let id = found.pr.id.clone();
            if !ids.contains(&id) && lookup.discovered.iter().all(|new| new.id != id) {
                lookup.discovered.push(Tracked {
                    id: id.clone(),
                    repo: branch.repo.clone(),
                    branch: branch.name.clone(),
                    verified: false,
                    head: None,
                    checked_head: None,
                    merged_at: None,
                    last: None,
                });
            }
            lookup.found.entry(id).or_insert(found);
        }
    }
    Ok(lookup)
}

fn names(value: &Value, expected: &str) -> bool {
    value
        .as_str()
        .is_some_and(|name| name.eq_ignore_ascii_case(expected))
}

fn found(node: &Value, now_secs: i64) -> Option<Found> {
    let mut pr = pull_request(node)?;
    pr.contributed = true;
    let merged_at =
        (node["state"] == "MERGED").then(|| timestamp(&node["mergedAt"]).unwrap_or(now_secs));
    Some(Found {
        pr,
        head: node["headRefOid"].as_str().map(str::to_string),
        merged_at,
    })
}

fn verify(github: &dyn Github, store: &mut Store, viewer: &str) -> Result<(), String> {
    let due: Vec<String> = store
        .tracked
        .iter()
        .filter(|tracked| {
            !tracked.verified && tracked.head.is_some() && tracked.head != tracked.checked_head
        })
        .map(|tracked| tracked.id.clone())
        .take(VERIFY_PER_READ)
        .collect();
    if due.is_empty() {
        return Ok(());
    }
    let reply = github.graphql(&json!({ "query": VERIFY, "variables": { "ids": due } }))?;
    let nodes = reply["data"]["nodes"]
        .as_array()
        .filter(|nodes| nodes.len() == due.len())
        .ok_or_else(|| first_error(&reply))?;
    for (id, node) in due.iter().zip(nodes) {
        if let Some(tracked) = store.tracked.iter_mut().find(|tracked| &tracked.id == id) {
            tracked.verified = wrote_a_commit(node, viewer);
            tracked.checked_head.clone_from(&tracked.head);
        }
    }
    Ok(())
}

fn wrote_a_commit(node: &Value, viewer: &str) -> bool {
    node["commits"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|node| &node["commit"])
        .filter(|commit| commit["parents"]["totalCount"].as_u64().unwrap_or(0) <= 1)
        .flat_map(|commit| commit["authors"]["nodes"].as_array().into_iter().flatten())
        .any(|author| names(&author["user"]["login"], viewer))
}

fn first_error(reply: &Value) -> String {
    reply["errors"][0]["message"].as_str().map_or_else(
        || "GitHub sent an unexpected reply.".to_string(),
        |message| format!("GitHub answered: {}.", message.trim_end_matches('.')),
    )
}

fn timestamp(value: &Value) -> Option<i64> {
    Some(
        DateTime::parse_from_rfc3339(value.as_str()?)
            .ok()?
            .timestamp(),
    )
}

fn load(path: &Path, viewer: &str) -> Store {
    let Some(mut store) = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Store>(&text).ok())
        .filter(|store| {
            store.schema_version == SCHEMA_VERSION && store.viewer.eq_ignore_ascii_case(viewer)
        })
    else {
        return Store::new(viewer);
    };
    for pr in store
        .tracked
        .iter_mut()
        .filter_map(|tracked| tracked.last.as_mut())
    {
        pr.merge_kind = merge::classify(pr);
    }
    store
}

fn save(path: &Path, store: &Store) -> Result<(), String> {
    let json = serde_json::to_string(store).map_err(|error| format!("{error}."))?;
    atomic_write(path, &json)
        .map_err(|error| format!("Could not write {}: {error}.", path.display()))
}

#[cfg(test)]
mod tests;
