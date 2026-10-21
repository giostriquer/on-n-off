use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::parse::pull_request;
use super::query::PR_FRAGMENT;
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
    fn events(&self, page: u32) -> Result<Value, String>;
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Contributed {
    pub(super) open: Vec<GithubPrDto>,
    pub(super) merged: Vec<GithubPrDto>,
    pub(super) complete: bool,
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
    checked_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    merged_at: Option<i64>,
}

struct Pushes {
    branches: Vec<Branch>,
    newest: Option<u64>,
}

struct Current {
    pr: GithubPrDto,
    head: Option<String>,
    merged_at: Option<i64>,
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
        }
    }

    fn remember(&mut self, pushes: Pushes, now_secs: i64) {
        for push in pushes.branches {
            match self
                .branches
                .iter_mut()
                .find(|known| known.repo == push.repo && known.name == push.name)
            {
                Some(known) => known.pushed_at = known.pushed_at.max(push.pushed_at),
                None => self.branches.push(push),
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
}

pub(super) fn read(
    github: &dyn Github,
    path: &Path,
    viewer: &str,
    scopes: &[String],
    now_secs: i64,
) -> Contributed {
    let mut store = load(path, viewer);
    let before = store.clone();
    let mut warnings = Vec::new();
    if now_secs - store.events_read_at >= EVENTS_EVERY_SECS {
        match read_pushes(github, store.newest_event) {
            Ok(pushes) => store.remember(pushes, now_secs),
            Err(why) => warnings.push(format!("Could not read your recent pushes ({why}).")),
        }
    }
    store.forget_old(now_secs);
    let mut contributed = match look_up(github, &mut store, viewer, now_secs) {
        Ok(current) => {
            if let Err(why) = verify(github, &mut store, &current, viewer) {
                warnings.push(format!(
                    "Could not check your commits on pull requests ({why})."
                ));
            }
            shown(&store, current, scopes)
        }
        Err(why) => {
            warnings.push(format!(
                "Could not look up the pull requests on your pushed branches ({why})."
            ));
            Contributed::default()
        }
    };
    if store != before {
        if let Err(why) = save(path, &store) {
            warnings.push(format!("Could not remember your pushed branches ({why})."));
        }
    }
    contributed.warnings = warnings;
    contributed
}

pub(super) fn in_scope(repo: &str, scopes: &[String]) -> bool {
    let owner = repo.split_once('/').map_or(repo, |(owner, _)| owner);
    scopes.is_empty()
        || scopes.iter().any(|scope| match scope.split_once(':') {
            Some(("org" | "user", login)) => owner.eq_ignore_ascii_case(login),
            Some(("repo", name)) => repo.eq_ignore_ascii_case(name),
            _ => false,
        })
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

fn read_pushes(github: &dyn Github, since: Option<u64>) -> Result<Pushes, String> {
    let mut pushes = Pushes {
        branches: Vec::new(),
        newest: None,
    };
    for page in 1..=EVENT_PAGES {
        let reply = github.events(page)?;
        let events = reply
            .as_array()
            .ok_or_else(|| "the events reply is not a list".to_string())?;
        for event in events {
            let Some(id) = event["id"].as_str().and_then(|id| id.parse::<u64>().ok()) else {
                continue;
            };
            if since.is_some_and(|since| id <= since) {
                return Ok(pushes);
            }
            pushes.newest = pushes.newest.max(Some(id));
            pushes.branches.extend(pushed_branch(event));
        }
        if events.len() < EVENTS_PER_PAGE {
            break;
        }
    }
    Ok(pushes)
}

fn pushed_branch(event: &Value) -> Option<Branch> {
    if event["type"] != "PushEvent" {
        return None;
    }
    let name = event["payload"]["ref"]
        .as_str()?
        .strip_prefix("refs/heads/")?;
    Some(Branch {
        repo: event["repo"]["name"].as_str()?.to_string(),
        name: name.to_string(),
        pushed_at: timestamp(&event["created_at"])?,
    })
}

fn look_up(
    github: &dyn Github,
    store: &mut Store,
    viewer: &str,
    now_secs: i64,
) -> Result<HashMap<String, Current>, String> {
    if store.branches.is_empty() && store.tracked.is_empty() {
        return Ok(HashMap::new());
    }
    let ids: Vec<String> = store
        .tracked
        .iter()
        .map(|tracked| tracked.id.clone())
        .collect();
    let reply = github.graphql(&lookup_body(&store.branches, &ids))?;
    let data = &reply["data"];
    let tracked_nodes = data["tracked"]
        .as_array()
        .filter(|nodes| nodes.len() == ids.len())
        .ok_or_else(|| first_error(&reply))?;

    let mut current = HashMap::new();
    let mut gone = HashSet::new();
    for (id, node) in ids.iter().zip(tracked_nodes) {
        match node["state"].as_str() {
            Some("OPEN" | "MERGED") => {
                if let Some(found) = current_of(node, now_secs) {
                    current.insert(id.clone(), found);
                }
            }
            _ => {
                gone.insert(id.clone());
            }
        }
    }

    let mut lost = HashSet::new();
    let mut discovered = Vec::new();
    for (index, branch) in store.branches.iter().enumerate() {
        let repository = &data[format!("b{index}")];
        if repository.is_null() {
            lost.insert(index);
            continue;
        }
        for node in repository["pullRequests"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let on_branch = node["headRepository"]["nameWithOwner"]
                .as_str()
                .is_some_and(|repo| repo.eq_ignore_ascii_case(&branch.repo));
            let opened_by_viewer = node["author"]["login"]
                .as_str()
                .is_some_and(|login| login.eq_ignore_ascii_case(viewer));
            if !on_branch || opened_by_viewer {
                continue;
            }
            let Some(found) = current_of(node, now_secs) else {
                continue;
            };
            let id = found.pr.id.clone();
            let known = ids.contains(&id) || discovered.iter().any(|new: &Tracked| new.id == id);
            if !known {
                discovered.push(Tracked {
                    id: id.clone(),
                    repo: branch.repo.clone(),
                    branch: branch.name.clone(),
                    verified: false,
                    checked_head: None,
                    merged_at: None,
                });
            }
            current.entry(id).or_insert(found);
        }
    }

    let mut index = 0;
    store.branches.retain(|_| {
        index += 1;
        !lost.contains(&(index - 1))
    });
    store.tracked.retain(|tracked| !gone.contains(&tracked.id));
    store.tracked.extend(discovered);
    for tracked in &mut store.tracked {
        if let Some(found) = current.get(&tracked.id) {
            tracked.merged_at = found.merged_at;
        }
    }
    Ok(current)
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
            "  b{index}: repository(owner: $o{index}, name: $n{index}) {{ \
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

fn current_of(node: &Value, now_secs: i64) -> Option<Current> {
    let mut pr = pull_request(node)?;
    pr.contributed = true;
    let merged_at =
        (node["state"] == "MERGED").then(|| timestamp(&node["mergedAt"]).unwrap_or(now_secs));
    Some(Current {
        pr,
        head: node["headRefOid"].as_str().map(str::to_string),
        merged_at,
    })
}

fn verify(
    github: &dyn Github,
    store: &mut Store,
    current: &HashMap<String, Current>,
    viewer: &str,
) -> Result<(), String> {
    let due: Vec<String> = store
        .tracked
        .iter()
        .filter(|tracked| !tracked.verified)
        .filter(|tracked| {
            current
                .get(&tracked.id)
                .is_some_and(|found| found.head != tracked.checked_head)
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
            tracked.checked_head = current.get(id).and_then(|found| found.head.clone());
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
        .any(|author| {
            author["user"]["login"]
                .as_str()
                .is_some_and(|login| login.eq_ignore_ascii_case(viewer))
        })
}

fn shown(store: &Store, mut current: HashMap<String, Current>, scopes: &[String]) -> Contributed {
    let mut contributed = Contributed {
        complete: true,
        ..Contributed::default()
    };
    for tracked in store.tracked.iter().filter(|tracked| tracked.verified) {
        let Some(found) = current.remove(&tracked.id) else {
            continue;
        };
        if !in_scope(&found.pr.repo, scopes) {
            continue;
        }
        if found.merged_at.is_some() {
            contributed.merged.push(found.pr);
        } else {
            contributed.open.push(found.pr);
        }
    }
    contributed
}

fn first_error(reply: &Value) -> String {
    reply["errors"][0]["message"]
        .as_str()
        .unwrap_or("unexpected reply")
        .to_string()
}

fn timestamp(value: &Value) -> Option<i64> {
    Some(
        DateTime::parse_from_rfc3339(value.as_str()?)
            .ok()?
            .timestamp(),
    )
}

fn load(path: &Path, viewer: &str) -> Store {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Store>(&text).ok())
        .filter(|store| {
            store.schema_version == SCHEMA_VERSION && store.viewer.eq_ignore_ascii_case(viewer)
        })
        .unwrap_or_else(|| Store::new(viewer))
}

fn save(path: &Path, store: &Store) -> Result<(), String> {
    let json = serde_json::to_string(store).map_err(|error| error.to_string())?;
    atomic_write(path, &json).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests;
