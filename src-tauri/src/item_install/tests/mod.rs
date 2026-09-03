use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use flate2::write::GzEncoder;
use flate2::Compression;

use super::fetch::{self, Fetcher};
use super::registry::{self, InstalledItem, InstalledItemsFile};
use super::{write, ItemService, Memo, ResolveRoots};
use crate::adapter::AgentAdapter;
use crate::antigravity::AntigravityAdapter;
use crate::backup::BackupStore;
use crate::claude::ClaudeAdapter;
use crate::codex::CodexAdapter;
use crate::cursor::CursorAdapter;
use crate::dto::{
    AgentId, ErrorKind, InstallItemsRequest, ItemKind, ItemOutcomeStatus, ItemPick, ItemScope,
    ItemSourceDto, ItemTarget, ItemUpstream, UpdateItemMode,
};
use crate::paths::scratch_dir;

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccc";

impl ItemService {
    pub fn at(home: PathBuf, fetcher: Box<dyn Fetcher>) -> Self {
        Self {
            registry_path: crate::paths::installed_items_path_for(&home),
            backups: BackupStore::at(home.join(".on-n-off").join("backups")),
            fetcher,
            memo: Mutex::new(Memo::default()),
            registry_lock: Mutex::new(()),
        }
    }
}

// Fixtures shared by every test module in this folder.

/// A GitHub-shaped tarball: pax global header carrying `comment=<sha>`, then every file
/// under a `<repo>-<something>/` top-level folder.
fn tarball_with_root(root: &str, sha: Option<&str>, files: &[(&str, &str)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    if let Some(sha) = sha {
        let record_body = format!("comment={sha}\n");
        // pax record: "<len> <key>=<value>\n" where len counts the whole record.
        let mut len = record_body.len() + 1;
        loop {
            let candidate = format!("{len} {record_body}");
            if candidate.len() == len {
                break;
            }
            len = candidate.len();
        }
        let record = format!("{len} {record_body}");
        let mut header = tar::Header::new_ustar();
        header.set_entry_type(tar::EntryType::XGlobalHeader);
        header.set_path("pax_global_header").unwrap();
        header.set_size(record.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, record.as_bytes()).unwrap();
    }
    for (path, contents) in files {
        let mut header = tar::Header::new_ustar();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        // Raw name bytes: `set_path` refuses `..`, which is exactly what one fixture needs.
        let name = format!("{root}/{path}");
        let ustar = header.as_ustar_mut().unwrap();
        ustar.name[..name.len()].copy_from_slice(name.as_bytes());
        header.set_cksum();
        builder.append(&header, contents.as_bytes()).unwrap();
    }
    let tar_bytes = builder.into_inner().unwrap();
    let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
    gz.write_all(&tar_bytes).unwrap();
    gz.finish().unwrap()
}

fn tarball(sha: &str, files: &[(&str, &str)]) -> Vec<u8> {
    tarball_with_root("skills-main", Some(sha), files)
}

fn marketplace_json(plugins: &[(&str, &str)]) -> String {
    let entries: Vec<String> = plugins
        .iter()
        .map(|(name, source)| format!(r#"{{"name":"{name}","source":"{source}"}}"#))
        .collect();
    format!(
        r#"{{"name":"mattpocock","plugins":[{}]}}"#,
        entries.join(",")
    )
}

fn skill_md(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n")
}

/// The canonical fixture: one plugin at `./`, `plugin.json` listing two skills, plus one agent.
fn mattpocock_files(version: &str, tdd_body: &str) -> Vec<(String, String)> {
    vec![
        (
            ".claude-plugin/marketplace.json".to_string(),
            marketplace_json(&[("mattpocock-skills", "./")]),
        ),
        (
            ".claude-plugin/plugin.json".to_string(),
            format!(
                r#"{{"name":"mattpocock-skills","version":"{version}","description":"Matt's skills","skills":["./skills/engineering/tdd","./skills/productivity/grilling"]}}"#
            ),
        ),
        (
            "skills/engineering/tdd/SKILL.md".to_string(),
            format!("{}{tdd_body}", skill_md("tdd", "Test-driven development")),
        ),
        (
            "skills/engineering/tdd/reference.md".to_string(),
            "red, green, refactor".to_string(),
        ),
        (
            "skills/engineering/tdd/ref/notes.md".to_string(),
            "nested".to_string(),
        ),
        (
            "skills/productivity/grilling/SKILL.md".to_string(),
            skill_md("grilling", "Ask hard questions"),
        ),
        (
            "agents/reviewer.md".to_string(),
            "---\nname: reviewer\ndescription: Reviews code\n---\nBe strict.\n".to_string(),
        ),
    ]
}

fn mattpocock_tarball(sha: &str, version: &str, tdd_body: &str) -> Vec<u8> {
    let files = mattpocock_files(version, tdd_body);
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    tarball(sha, &borrowed)
}

#[derive(Default)]
struct FakeFetcher {
    routes: Mutex<HashMap<String, Result<Vec<u8>, String>>>,
    calls: Mutex<Vec<String>>,
}

impl FakeFetcher {
    fn route(&self, url: &str, body: Vec<u8>) {
        self.routes
            .lock()
            .unwrap()
            .insert(url.to_string(), Ok(body));
    }

    fn fail(&self, url: &str, message: &str) {
        self.routes
            .lock()
            .unwrap()
            .insert(url.to_string(), Err(message.to_string()));
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Fetcher for FakeFetcher {
    fn get(&self, url: &str, _accept: Option<&str>) -> Result<Vec<u8>, String> {
        self.calls.lock().unwrap().push(url.to_string());
        self.routes
            .lock()
            .unwrap()
            .get(url)
            .cloned()
            .unwrap_or_else(|| Err(format!("unrouted url {url}")))
    }
}

/// A fetcher shared between the test and the service (the service owns a `Box<dyn Fetcher>`).
struct SharedFetcher(std::sync::Arc<FakeFetcher>);

impl Fetcher for SharedFetcher {
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Vec<u8>, String> {
        self.0.get(url, accept)
    }
}

struct Harness {
    home: PathBuf,
    fetcher: std::sync::Arc<FakeFetcher>,
    service: ItemService,
}

impl Harness {
    fn new(prefix: &str) -> Self {
        let home = scratch_dir(prefix);
        let fetcher = std::sync::Arc::new(FakeFetcher::default());
        let service = ItemService::at(
            home.clone(),
            Box::new(SharedFetcher(std::sync::Arc::clone(&fetcher))),
        );
        Self {
            home,
            fetcher,
            service,
        }
    }

    fn route_repo(&self, git_ref: &str, sha: &str, body: Vec<u8>) {
        self.fetcher
            .route(&fetch::tarball_url("mattpocock", "skills", git_ref), body);
        self.fetcher.route(
            &fetch::commit_sha_url("mattpocock", "skills", git_ref),
            sha.as_bytes().to_vec(),
        );
    }

    fn claude(&self) -> ClaudeAdapter {
        ClaudeAdapter::at(self.home.join(".claude"))
    }

    fn codex(&self) -> CodexAdapter {
        CodexAdapter::at(self.home.join(".codex"), self.home.join(".agents/skills"))
    }

    /// Resolves targets through the real adapters rooted in this scratch home.
    fn resolver(
        &self,
    ) -> impl Fn(&ItemTarget) -> Result<crate::adapter::ItemRoots, crate::dto::AdapterError> + '_
    {
        move |target: &ItemTarget| match target.provider {
            AgentId::Claude => self.claude().item_roots(&target.scope),
            AgentId::Codex => self.codex().item_roots(&target.scope),
            AgentId::Antigravity => {
                AntigravityAdapter::at(self.home.join(".gemini")).item_roots(&target.scope)
            }
            AgentId::Cursor => {
                CursorAdapter::at(self.home.join(".cursor")).item_roots(&target.scope)
            }
        }
    }

    /// Runs `install_items` against this home for the given targets.
    fn install(
        &self,
        mut request: InstallItemsRequest,
        targets: Vec<(AgentId, ItemScope)>,
    ) -> Result<crate::dto::InstallItemsResultDto, crate::dto::AdapterError> {
        request.targets = targets
            .into_iter()
            .map(|(provider, scope)| ItemTarget { provider, scope })
            .collect();
        let resolve = self.resolver();
        let resolve: ResolveRoots<'_> = &resolve;
        self.service.install_items(request, resolve)
    }

    fn registry(&self) -> InstalledItemsFile {
        registry::load(&crate::paths::installed_items_path_for(&self.home)).unwrap()
    }

    fn finish(self) {
        let _ = fs::remove_dir_all(self.home);
    }
}

fn source() -> ItemSourceDto {
    ItemSourceDto {
        owner: "mattpocock".into(),
        repo: "skills".into(),
        git_ref: "HEAD".into(),
    }
}

fn pick(kind: ItemKind, path: &str) -> ItemPick {
    ItemPick {
        plugin_name: "mattpocock-skills".into(),
        kind,
        path: path.into(),
        source: None,
    }
}

fn request(commit_sha: &str, items: Vec<ItemPick>, overwrite: bool) -> InstallItemsRequest {
    InstallItemsRequest {
        source: source(),
        commit_sha: commit_sha.into(),
        items,
        targets: Vec::new(),
        overwrite_unmanaged: overwrite,
    }
}

fn sample_item(home: &Path, name: &str) -> InstalledItem {
    let target = home.join(".claude/skills").join(name);
    InstalledItem {
        id: registry::item_id(AgentId::Claude, ItemKind::Skill, &target),
        provider: AgentId::Claude,
        kind: ItemKind::Skill,
        name: name.into(),
        target_path: target.to_string_lossy().into_owned(),
        scope: ItemScope::Global,
        source: registry::ItemSource {
            owner: "mattpocock".into(),
            repo: "skills".into(),
            git_ref: "HEAD".into(),
            plugin_name: "mattpocock-skills".into(),
            plugin_root: String::new(),
            upstream_path: format!("skills/engineering/{name}"),
            depends_on: Vec::new(),
        },
        installed: registry::Installed {
            commit_sha: SHA_A.into(),
            plugin_version: Some("1.2.3".into()),
            installed_at: "2026-08-18T00:00:00Z".into(),
        },
        files: BTreeMap::new(),
        dismissed_sha: None,
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

mod deps;
mod inspect;
mod install;
mod real_github;
mod roots;
mod status;
mod store;
mod unpack;
