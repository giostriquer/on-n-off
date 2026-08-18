use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use flate2::write::GzEncoder;
use flate2::Compression;

use super::fetch::{self, Fetcher};
use super::registry::{self, InstalledItem, InstalledItemsFile};
use super::{write, ItemService, ResolvedTarget};
use crate::adapter::AgentAdapter;
use crate::antigravity::AntigravityAdapter;
use crate::claude::ClaudeAdapter;
use crate::codex::CodexAdapter;
use crate::cursor::CursorAdapter;
use crate::dto::{
    AgentId, ErrorKind, InstallItemsRequest, ItemKind, ItemOutcomeStatus, ItemPick, ItemScope,
    ItemSourceDto, ItemUpstream, UpdateItemMode,
};
use crate::paths::scratch_dir;

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

// ---------------------------------------------------------------------------
// Fixtures

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

    fn target(&self, provider: AgentId, scope: ItemScope) -> ResolvedTarget {
        let roots = match provider {
            AgentId::Claude => self.claude().item_roots(&scope),
            AgentId::Codex => self.codex().item_roots(&scope),
            AgentId::Antigravity => {
                AntigravityAdapter::at(self.home.join(".gemini")).item_roots(&scope)
            }
            AgentId::Cursor => CursorAdapter::at(self.home.join(".cursor")).item_roots(&scope),
        }
        .unwrap();
        ResolvedTarget {
            provider,
            scope,
            roots,
        }
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

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

// ---------------------------------------------------------------------------
// fetch.rs

#[test]
fn unpack_reads_commit_sha_and_strips_top_level_folder() {
    let bytes = tarball_with_root(
        "skills-3f2a1b",
        Some(SHA_A),
        &[("README.md", "hello"), ("skills/tdd/SKILL.md", "body")],
    );
    let tarball = fetch::unpack(&bytes).unwrap();
    assert_eq!(tarball.commit_sha, SHA_A);
    assert_eq!(tarball.files.get("README.md").unwrap(), b"hello");
    assert_eq!(tarball.files.get("skills/tdd/SKILL.md").unwrap(), b"body");
    assert!(!tarball.files.keys().any(|k| k.starts_with("skills-3f2a1b")));
}

#[test]
fn unpack_rejects_missing_commit_sha() {
    let bytes = tarball_with_root("r-x", None, &[("a", "b")]);
    let error = fetch::unpack(&bytes).unwrap_err();
    assert!(error.message.contains("commit"), "{error:?}");
}

#[test]
fn unpack_rejects_path_traversal() {
    let bytes = tarball_with_root(
        "r-x",
        Some(SHA_A),
        &[("../../evil.md", "x"), ("ok.md", "y")],
    );
    let error = fetch::unpack(&bytes).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Parse);
}

#[test]
fn unpack_skips_symlinks_and_directories() {
    let mut builder = tar::Builder::new(Vec::new());
    let mut dir = tar::Header::new_ustar();
    dir.set_entry_type(tar::EntryType::Directory);
    dir.set_size(0);
    dir.set_mode(0o755);
    dir.set_cksum();
    builder
        .append_data(&mut dir, "r-x/skills/", &[][..])
        .unwrap();
    let mut link = tar::Header::new_ustar();
    link.set_entry_type(tar::EntryType::Symlink);
    link.set_size(0);
    link.set_mode(0o644);
    link.set_link_name("../../etc/passwd").unwrap();
    link.set_cksum();
    builder
        .append_data(&mut link, "r-x/skills/link", &[][..])
        .unwrap();
    let mut file = tar::Header::new_ustar();
    file.set_size(1);
    file.set_mode(0o644);
    file.set_cksum();
    builder
        .append_data(&mut file, "r-x/skills/a.md", &b"a"[..])
        .unwrap();
    let mut pax = tar::Header::new_ustar();
    let record = format!("{} comment={SHA_A}\n", 9 + SHA_A.len() + 1 + 2);
    pax.set_entry_type(tar::EntryType::XGlobalHeader);
    pax.set_size(record.len() as u64);
    pax.set_cksum();
    let mut all = tar::Builder::new(Vec::new());
    all.append_data(&mut pax, "pax_global_header", record.as_bytes())
        .unwrap();
    let rest = builder.into_inner().unwrap();
    let mut merged = all.into_inner().unwrap();
    // Drop the trailing 1024-byte end-of-archive marker of the first builder before appending.
    merged.truncate(merged.len() - 1024);
    merged.extend_from_slice(&rest);
    let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
    gz.write_all(&merged).unwrap();
    let bytes = gz.finish().unwrap();

    let tarball = fetch::unpack(&bytes).unwrap();
    assert_eq!(tarball.commit_sha, SHA_A);
    assert_eq!(tarball.files.len(), 1);
    assert!(tarball.files.contains_key("skills/a.md"));
}

#[test]
fn unpack_rejects_oversized_bodies() {
    let too_big = vec![0u8; fetch::MAX_TARBALL_BYTES + 1];
    let error = fetch::unpack(&too_big).unwrap_err();
    assert!(error.message.contains("large"), "{error:?}");
}

#[test]
fn upstream_sha_validates_and_trims() {
    let fetcher = FakeFetcher::default();
    let url = fetch::commit_sha_url("o", "r", "main");
    fetcher.route(&url, format!("{SHA_A}\n").into_bytes());
    assert_eq!(
        fetch::upstream_sha(&fetcher, "o", "r", "main").unwrap(),
        SHA_A
    );
    fetcher.route(&url, b"<html>rate limited</html>".to_vec());
    assert!(fetch::upstream_sha(&fetcher, "o", "r", "main").is_err());
}

// ---------------------------------------------------------------------------
// registry.rs

#[test]
fn registry_round_trips_and_upserts_by_id() {
    let home = scratch_dir("items-registry");
    let path = crate::paths::installed_items_path_for(&home);
    assert!(registry::load(&path).unwrap().items.is_empty());
    let mut file = InstalledItemsFile::default();
    let mut item = sample_item(&home, "tdd");
    file.upsert(item.clone());
    item.installed.commit_sha = SHA_B.into();
    file.upsert(item.clone());
    assert_eq!(file.items.len(), 1);
    assert_eq!(file.items[0].installed.commit_sha, SHA_B);
    registry::save(&path, &file).unwrap();
    let mut loaded = registry::load(&path).unwrap();
    assert_eq!(loaded, file);
    assert!(loaded.remove(&item.id).is_some());
    let _ = fs::remove_dir_all(home);
}

#[test]
fn registry_never_resets_a_malformed_file() {
    let home = scratch_dir("items-registry-bad");
    let path = crate::paths::installed_items_path_for(&home);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{ not json").unwrap();
    let error = registry::load(&path).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Parse);
    assert_eq!(read(&path), "{ not json");
    let _ = fs::remove_dir_all(home);
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

// ---------------------------------------------------------------------------
// write.rs

#[test]
fn place_replaces_atomically_and_keeps_old_copy_on_failure() {
    let root = scratch_dir("items-place");
    let dest = root.join("tdd");
    let mut files = BTreeMap::new();
    files.insert("SKILL.md".to_string(), b"v1".to_vec());
    files.insert("ref/a.md".to_string(), b"a".to_vec());
    write::place(&dest, ItemKind::Skill, &files).unwrap();
    assert_eq!(read(&dest.join("SKILL.md")), "v1");
    assert_eq!(read(&dest.join("ref/a.md")), "a");

    files.insert("SKILL.md".to_string(), b"v2".to_vec());
    let result = write::place_with(&dest, ItemKind::Skill, &files, || {
        Err(std::io::Error::other("injected"))
    });
    assert!(result.is_err());
    assert_eq!(read(&dest.join("SKILL.md")), "v1");
    let leftovers: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, vec!["tdd".to_string()], "{leftovers:?}");

    write::place(&dest, ItemKind::Skill, &files).unwrap();
    assert_eq!(read(&dest.join("SKILL.md")), "v2");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn hash_tree_detects_drift_and_missing() {
    let root = scratch_dir("items-hash");
    let dest = root.join("tdd");
    let mut files = BTreeMap::new();
    files.insert("SKILL.md".to_string(), b"v1".to_vec());
    let expected = write::hash_files(&files);
    assert!(write::hash_tree_on_disk(&dest, ItemKind::Skill)
        .unwrap()
        .is_none());
    write::place(&dest, ItemKind::Skill, &files).unwrap();
    assert_eq!(
        write::hash_tree_on_disk(&dest, ItemKind::Skill)
            .unwrap()
            .unwrap(),
        expected
    );
    fs::write(dest.join("SKILL.md"), "edited").unwrap();
    assert_ne!(
        write::hash_tree_on_disk(&dest, ItemKind::Skill)
            .unwrap()
            .unwrap(),
        expected
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn item_files_extracts_folder_or_single_file_and_rejects_escapes() {
    let bytes = mattpocock_tarball(SHA_A, "1.2.3", "");
    let tarball = fetch::unpack(&bytes).unwrap();
    let skill = write::item_files(&tarball, "skills/engineering/tdd", ItemKind::Skill).unwrap();
    assert_eq!(
        skill.keys().cloned().collect::<Vec<_>>(),
        vec!["SKILL.md".to_string(), "reference.md".to_string()]
    );
    let agent = write::item_files(&tarball, "agents/reviewer.md", ItemKind::Agent).unwrap();
    assert_eq!(
        agent.keys().cloned().collect::<Vec<_>>(),
        vec!["reviewer.md".to_string()]
    );
    let win = write::item_files(&tarball, r"skills\engineering\tdd", ItemKind::Skill).unwrap();
    assert_eq!(win.len(), 2);
    assert!(write::item_files(&tarball, "../x", ItemKind::Skill).is_err());
    assert!(write::item_files(&tarball, "skills/nope", ItemKind::Skill).is_err());
}

// ---------------------------------------------------------------------------
// manifest.rs — via ItemService::inspect_marketplace

#[test]
fn inspect_lists_plugins_skills_and_agents_from_plugin_json() {
    let h = Harness::new("items-inspect");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let dto = h
        .service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap();
    assert!(dto.is_marketplace);
    assert_eq!(dto.commit_sha, SHA_A);
    assert_eq!(dto.marketplace_name, "mattpocock");
    assert_eq!(dto.plugins.len(), 1);
    let plugin = &dto.plugins[0];
    assert_eq!(plugin.name, "mattpocock-skills");
    assert_eq!(plugin.version.as_deref(), Some("1.2.3"));
    assert!(plugin.supported);
    let skills: Vec<(&str, &str)> = plugin
        .skills
        .iter()
        .map(|s| (s.name.as_str(), s.path.as_str()))
        .collect();
    assert_eq!(
        skills,
        vec![
            ("tdd", "skills/engineering/tdd"),
            ("grilling", "skills/productivity/grilling")
        ]
    );
    assert_eq!(plugin.skills[0].description, "Test-driven development");
    assert_eq!(plugin.agents.len(), 1);
    assert_eq!(plugin.agents[0].name, "reviewer");
    assert_eq!(plugin.agents[0].path, "agents/reviewer.md");
    // memoised: a second inspect makes no further network calls
    let calls = h.fetcher.calls().len();
    h.service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap();
    assert_eq!(h.fetcher.calls().len(), calls);
    h.finish();
}

#[test]
fn inspect_falls_back_to_scanning_skills_folder_and_flags_unsupported_sources() {
    let h = Harness::new("items-inspect-scan");
    let files = [
        (
            ".claude-plugin/marketplace.json",
            r#"{"name":"mkt","plugins":[{"name":"local","source":"./plugins/local"},{"name":"remote","source":{"source":"url","url":"https://example.com/x.git"}},{"name":"gh","source":{"source":"github","repo":"other/repo"}}]}"#,
        ),
        (
            "plugins/local/.claude-plugin/plugin.json",
            r#"{"name":"local"}"#,
        ),
        (
            "plugins/local/skills/alpha/SKILL.md",
            "---\nname: alpha\n---\n",
        ),
        ("plugins/local/skills/beta/SKILL.md", "no frontmatter"),
        ("plugins/local/skills/notes.md", "stray file"),
    ];
    let bytes = tarball(SHA_A, &files);
    h.route_repo("HEAD", SHA_A, bytes);
    h.fetcher.route(
        &fetch::tarball_url("other", "repo", "HEAD"),
        tarball_with_root(
            "repo-main",
            Some(SHA_B),
            &[("skills/gamma/SKILL.md", "---\nname: gamma\n---\n")],
        ),
    );
    let dto = h
        .service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap();
    assert_eq!(dto.plugins.len(), 3);
    let local = &dto.plugins[0];
    assert!(local.supported);
    let names: Vec<&str> = local.skills.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta"]);
    assert_eq!(local.skills[0].path, "plugins/local/skills/alpha");
    let remote = &dto.plugins[1];
    assert!(!remote.supported);
    assert!(remote.skills.is_empty());
    let gh = &dto.plugins[2];
    assert!(gh.supported);
    assert_eq!(gh.skills[0].name, "gamma");
    assert_eq!(gh.skills[0].path, "skills/gamma");
    assert_eq!(
        gh.source
            .as_ref()
            .map(|s| (s.owner.as_str(), s.repo.as_str())),
        Some(("other", "repo"))
    );
    h.finish();
}

#[test]
fn inspect_reports_non_marketplace_repos() {
    let h = Harness::new("items-inspect-none");
    h.route_repo("HEAD", SHA_A, tarball(SHA_A, &[("README.md", "hi")]));
    let dto = h
        .service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap();
    assert!(!dto.is_marketplace);
    assert!(dto.plugins.is_empty());
    assert!(dto.hint.is_some());
    h.finish();
}

#[test]
fn inspect_surfaces_download_failure() {
    let h = Harness::new("items-inspect-fail");
    h.fetcher
        .fail(&fetch::tarball_url("mattpocock", "skills", "HEAD"), "boom");
    let error = h
        .service
        .inspect_marketplace("mattpocock", "skills", None)
        .unwrap_err();
    assert!(error.message.contains("boom"), "{error:?}");
    h.finish();
}

// ---------------------------------------------------------------------------
// install_items

#[test]
fn install_writes_skill_and_records_it() {
    let h = Harness::new("items-install");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let mut req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    req.targets = vec![crate::dto::ItemTarget {
        provider: AgentId::Claude,
        scope: ItemScope::Global,
    }];
    let targets = vec![h.target(AgentId::Claude, ItemScope::Global)];
    let result = h.service.install_items(req, targets).unwrap();
    assert_eq!(result.commit_sha, SHA_A);
    assert!(!result.sha_moved);
    assert_eq!(result.outcomes.len(), 1);
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Installed);
    let dest = h.home.join(".claude/skills/tdd");
    assert!(read(&dest.join("SKILL.md")).contains("name: tdd"));
    assert_eq!(read(&dest.join("reference.md")), "red, green, refactor");
    let reg = h.registry();
    assert_eq!(reg.items.len(), 1);
    let item = &reg.items[0];
    assert_eq!(item.installed.commit_sha, SHA_A);
    assert_eq!(item.installed.plugin_version.as_deref(), Some("1.2.3"));
    assert_eq!(item.source.upstream_path, "skills/engineering/tdd");
    assert_eq!(item.files.len(), 2);
    assert!(item.files.contains_key("SKILL.md"));
    // Claude sees it as a user skill.
    let tab = h.claude().list_tab().unwrap();
    assert!(tab.user_skills.iter().any(|s| s.name == "tdd"));
    h.finish();
}

#[test]
fn install_into_several_providers_and_skips_agents_outside_claude() {
    let h = Harness::new("items-install-multi");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let req = request(
        SHA_A,
        vec![
            pick(ItemKind::Skill, "skills/engineering/tdd"),
            pick(ItemKind::Agent, "agents/reviewer.md"),
        ],
        false,
    );
    let targets = vec![
        h.target(AgentId::Claude, ItemScope::Global),
        h.target(AgentId::Codex, ItemScope::Global),
        h.target(AgentId::Antigravity, ItemScope::Global),
        h.target(AgentId::Cursor, ItemScope::Global),
    ];
    let result = h.service.install_items(req, targets).unwrap();
    let statuses: Vec<(AgentId, ItemKind, ItemOutcomeStatus)> = result
        .outcomes
        .iter()
        .map(|o| (o.provider, o.kind, o.status))
        .collect();
    assert_eq!(
        statuses,
        vec![
            (
                AgentId::Claude,
                ItemKind::Skill,
                ItemOutcomeStatus::Installed
            ),
            (
                AgentId::Claude,
                ItemKind::Agent,
                ItemOutcomeStatus::Installed
            ),
            (
                AgentId::Codex,
                ItemKind::Skill,
                ItemOutcomeStatus::Installed
            ),
            (AgentId::Codex, ItemKind::Agent, ItemOutcomeStatus::Skipped),
            (
                AgentId::Antigravity,
                ItemKind::Skill,
                ItemOutcomeStatus::Installed
            ),
            (
                AgentId::Antigravity,
                ItemKind::Agent,
                ItemOutcomeStatus::Skipped
            ),
            (
                AgentId::Cursor,
                ItemKind::Skill,
                ItemOutcomeStatus::Installed
            ),
            (AgentId::Cursor, ItemKind::Agent, ItemOutcomeStatus::Skipped),
        ]
    );
    assert!(h.home.join(".claude/agents/reviewer.md").is_file());
    assert!(h.home.join(".codex/skills/tdd/SKILL.md").is_file());
    assert!(h
        .home
        .join(".gemini/antigravity-cli/skills/tdd/SKILL.md")
        .is_file());
    assert!(h.home.join(".cursor/skills/tdd/SKILL.md").is_file());
    let anti = AntigravityAdapter::at(h.home.join(".gemini"))
        .list_tab()
        .unwrap();
    assert!(anti.user_skills.iter().any(|s| s.name == "tdd"));
    assert_eq!(h.registry().items.len(), 5);
    // Only one tarball download for the whole batch.
    let tarballs = h
        .fetcher
        .calls()
        .iter()
        .filter(|u| u.contains("codeload"))
        .count();
    assert_eq!(tarballs, 1);
    h.finish();
}

#[test]
fn install_into_project_scope_creates_provider_dirs() {
    let h = Harness::new("items-install-project");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let project = h.home.join("proj");
    fs::create_dir_all(&project).unwrap();
    let scope = ItemScope::Project {
        project_path: project.to_string_lossy().into_owned(),
    };
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    let targets = vec![
        h.target(AgentId::Claude, scope.clone()),
        h.target(AgentId::Codex, scope.clone()),
    ];
    let result = h.service.install_items(req, targets).unwrap();
    assert!(result
        .outcomes
        .iter()
        .all(|o| o.status == ItemOutcomeStatus::Installed));
    assert!(project.join(".claude/skills/tdd/SKILL.md").is_file());
    assert!(project.join(".codex/skills/tdd/SKILL.md").is_file());
    assert!(h.registry().items.iter().all(|i| i.scope == scope));

    // A project folder that does not exist fails without creating anything.
    let missing = ItemScope::Project {
        project_path: h.home.join("nope").to_string_lossy().into_owned(),
    };
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    let result = h
        .service
        .install_items(req, vec![h.target(AgentId::Claude, missing)])
        .unwrap();
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Failed);
    assert!(!h.home.join("nope").exists());
    h.finish();
}

#[test]
fn install_conflicts_with_unmanaged_folder_unless_overwrite() {
    let h = Harness::new("items-install-conflict");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let dest = h.home.join(".claude/skills/tdd");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("SKILL.md"), "mine").unwrap();
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    let result = h
        .service
        .install_items(req, vec![h.target(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Conflict);
    assert_eq!(read(&dest.join("SKILL.md")), "mine");
    assert!(h.registry().items.is_empty());

    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        true,
    );
    let result = h
        .service
        .install_items(req, vec![h.target(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Replaced);
    assert!(read(&dest.join("SKILL.md")).contains("name: tdd"));
    let backups = h.home.join(".on-n-off/backups/claude/items");
    assert!(backups.is_dir(), "backup of the unmanaged folder is kept");
    assert_eq!(h.registry().items.len(), 1);
    h.finish();
}

#[test]
fn install_replaces_managed_item_with_backup_and_dedupes_batch() {
    let h = Harness::new("items-install-replace");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    h.service
        .install_items(req, vec![h.target(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    // Same skill twice in one batch (e.g. two plugins shipping `tdd`): second is a conflict.
    let mut second = pick(ItemKind::Skill, "skills/engineering/tdd");
    second.plugin_name = "other-plugin".into();
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd"), second],
        false,
    );
    let result = h
        .service
        .install_items(req, vec![h.target(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Replaced);
    assert_eq!(result.outcomes[1].status, ItemOutcomeStatus::Conflict);
    assert_eq!(h.registry().items.len(), 1);
    let backups = fs::read_dir(h.home.join(".on-n-off/backups/claude/items"))
        .unwrap()
        .count();
    assert_eq!(backups, 1);
    h.finish();
}

#[test]
fn install_refetches_when_sha_moved_and_reports_it() {
    let h = Harness::new("items-install-moved");
    h.route_repo("HEAD", SHA_B, mattpocock_tarball(SHA_B, "1.3.0", "\nnew"));
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    let result = h
        .service
        .install_items(req, vec![h.target(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert!(result.sha_moved);
    assert_eq!(result.commit_sha, SHA_B);
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Installed);
    assert_eq!(h.registry().items[0].installed.commit_sha, SHA_B);
    h.finish();
}

#[test]
fn install_with_no_items_does_nothing() {
    let h = Harness::new("items-install-empty");
    let req = request(SHA_A, Vec::new(), false);
    let result = h
        .service
        .install_items(req, vec![h.target(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert!(result.outcomes.is_empty());
    assert!(h.fetcher.calls().is_empty());
    assert!(h.registry().items.is_empty());
    h.finish();
}

#[test]
fn install_reports_missing_upstream_path_as_failed() {
    let h = Harness::new("items-install-missing-path");
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/gone")],
        false,
    );
    let result = h
        .service
        .install_items(req, vec![h.target(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Failed);
    assert!(h.registry().items.is_empty());
    h.finish();
}

// ---------------------------------------------------------------------------
// item_update_status / update_item / remove_item

fn install_tdd(h: &Harness) -> String {
    h.route_repo("HEAD", SHA_A, mattpocock_tarball(SHA_A, "1.2.3", ""));
    let req = request(
        SHA_A,
        vec![pick(ItemKind::Skill, "skills/engineering/tdd")],
        false,
    );
    let result = h
        .service
        .install_items(req, vec![h.target(AgentId::Claude, ItemScope::Global)])
        .unwrap();
    assert_eq!(result.outcomes[0].status, ItemOutcomeStatus::Installed);
    h.registry().items[0].id.clone()
}

#[test]
fn status_reports_current_modified_and_missing() {
    let h = Harness::new("items-status");
    install_tdd(&h);
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, false)
        .unwrap();
    assert_eq!(statuses.len(), 1);
    let s = &statuses[0];
    assert_eq!(s.name, "tdd");
    assert_eq!(s.display_name, "tdd");
    assert_eq!(s.installed_version.as_deref(), Some("1.2.3"));
    assert!(!s.modified);
    assert!(!s.missing);
    assert_eq!(s.upstream, ItemUpstream::Current);

    let dest = h.home.join(".claude/skills/tdd");
    fs::write(dest.join("SKILL.md"), "---\nname: my-tdd\n---\nedited").unwrap();
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, false)
        .unwrap();
    assert!(statuses[0].modified);
    assert_eq!(statuses[0].display_name, "my-tdd");

    fs::remove_dir_all(&dest).unwrap();
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, false)
        .unwrap();
    assert!(statuses[0].missing);
    // Other providers / scopes see nothing.
    assert!(h
        .service
        .item_update_status(AgentId::Codex, None, false)
        .unwrap()
        .is_empty());
    assert!(h
        .service
        .item_update_status(AgentId::Claude, Some("/some/project"), false)
        .unwrap()
        .is_empty());
    h.finish();
}

#[test]
fn status_detects_upstream_update_and_honours_dismiss() {
    let h = Harness::new("items-status-update");
    let id = install_tdd(&h);
    // Upstream advances with a changed skill.
    h.route_repo("HEAD", SHA_B, mattpocock_tarball(SHA_B, "1.3.0", "\nnew"));
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    assert_eq!(
        statuses[0].upstream,
        ItemUpstream::UpdateAvailable {
            commit_sha: SHA_B.into(),
            plugin_version: Some("1.3.0".into()),
        }
    );
    // Keep mine → dismissed for this sha only.
    let dismissed = h.service.update_item(&id, UpdateItemMode::Dismiss).unwrap();
    assert_eq!(dismissed.upstream, ItemUpstream::Current);
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    assert_eq!(statuses[0].upstream, ItemUpstream::Current);
    // Overwrite → files replaced, backup kept, registry moved to SHA_B.
    let updated = h
        .service
        .update_item(&id, UpdateItemMode::Overwrite)
        .unwrap();
    assert_eq!(updated.installed_sha, SHA_B);
    assert_eq!(updated.installed_version.as_deref(), Some("1.3.0"));
    assert!(read(&h.home.join(".claude/skills/tdd/SKILL.md")).ends_with("new"));
    assert!(h.home.join(".on-n-off/backups/claude/items").is_dir());
    h.finish();
}

#[test]
fn status_treats_untouched_item_as_current_when_only_other_files_changed() {
    let h = Harness::new("items-status-same");
    install_tdd(&h);
    // Upstream advanced but tdd is byte-identical.
    h.route_repo("HEAD", SHA_B, mattpocock_tarball(SHA_B, "1.3.0", ""));
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    assert_eq!(statuses[0].upstream, ItemUpstream::Current);
    assert_eq!(
        statuses[0].installed_sha, SHA_B,
        "recorded sha moves forward"
    );
    h.finish();
}

#[test]
fn status_is_unknown_when_upstream_unreachable() {
    let h = Harness::new("items-status-offline");
    install_tdd(&h);
    h.fetcher.fail(
        &fetch::commit_sha_url("mattpocock", "skills", "HEAD"),
        "offline",
    );
    let statuses = h
        .service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    assert_eq!(statuses[0].upstream, ItemUpstream::Unknown);
    h.finish();
}

#[test]
fn status_uses_memoised_sha_unless_forced() {
    let h = Harness::new("items-status-memo");
    install_tdd(&h);
    let before = h.fetcher.calls().len();
    h.service
        .item_update_status(AgentId::Claude, None, false)
        .unwrap();
    assert_eq!(
        h.fetcher.calls().len(),
        before,
        "install already learned the sha"
    );
    h.service
        .item_update_status(AgentId::Claude, None, true)
        .unwrap();
    assert_eq!(h.fetcher.calls().len(), before + 1);
    h.finish();
}

#[test]
fn remove_item_deletes_folder_and_registry_row() {
    let h = Harness::new("items-remove");
    let id = install_tdd(&h);
    h.service.remove_item(&id).unwrap();
    assert!(!h.home.join(".claude/skills/tdd").exists());
    assert!(h.registry().items.is_empty());
    assert!(h.home.join(".on-n-off/backups/claude/items").is_dir());
    assert!(h.service.remove_item(&id).is_err());
    h.finish();
}

// ---------------------------------------------------------------------------
// item_roots

#[test]
fn item_roots_per_provider_and_scope() {
    let home = scratch_dir("items-roots");
    let project = home.join("proj");
    let scope = ItemScope::Project {
        project_path: project.to_string_lossy().into_owned(),
    };
    let claude = ClaudeAdapter::at(home.join(".claude"));
    let g = claude.item_roots(&ItemScope::Global).unwrap();
    assert_eq!(g.skills, home.join(".claude/skills"));
    assert_eq!(g.agents, Some(home.join(".claude/agents")));
    let p = claude.item_roots(&scope).unwrap();
    assert_eq!(p.skills, project.join(".claude/skills"));
    assert_eq!(p.agents, Some(project.join(".claude/agents")));

    let codex = CodexAdapter::at(home.join(".codex"), home.join(".agents/skills"));
    assert_eq!(
        codex.item_roots(&ItemScope::Global).unwrap().skills,
        home.join(".codex/skills")
    );
    assert_eq!(
        codex.item_roots(&scope).unwrap().skills,
        project.join(".codex/skills")
    );
    assert!(codex.item_roots(&scope).unwrap().agents.is_none());

    let anti = AntigravityAdapter::at(home.join(".gemini"));
    assert_eq!(
        anti.item_roots(&ItemScope::Global).unwrap().skills,
        home.join(".gemini/antigravity-cli/skills")
    );
    assert_eq!(
        anti.item_roots(&scope).unwrap().skills,
        project.join(".agents/skills")
    );

    let cursor = CursorAdapter::at(home.join(".cursor"));
    assert_eq!(
        cursor.item_roots(&ItemScope::Global).unwrap().skills,
        home.join(".cursor/skills")
    );
    assert_eq!(
        cursor.item_roots(&scope).unwrap().skills,
        project.join(".cursor/skills")
    );
    let _ = fs::remove_dir_all(home);
}
