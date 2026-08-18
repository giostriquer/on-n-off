//! GitHub tarball download and in-memory unpacking.
//!
//! One `codeload.github.com` tarball gives us the whole repository plus the exact commit sha
//! (in the pax global header GitHub writes) with a single unauthenticated request. The update
//! check asks the REST API for the ref's sha (`Accept: application/vnd.github.sha`) and only
//! re-downloads when it moved.

use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;

use flate2::read::GzDecoder;

use crate::dto::AdapterError;

/// Compressed download cap; a skills repository is a few hundred kB.
pub const MAX_TARBALL_BYTES: usize = 50 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 200 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);

pub trait Fetcher: Send + Sync {
    /// Body bytes for a GET, or a human-readable failure.
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Vec<u8>, String>;
}

pub struct HttpFetcher;

impl Fetcher for HttpFetcher {
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Vec<u8>, String> {
        let agent = ureq::AgentBuilder::new()
            .timeout(TIMEOUT)
            .user_agent(concat!("on-n-off/", env!("CARGO_PKG_VERSION")))
            .build();
        let mut request = agent.get(url);
        if let Some(accept) = accept {
            request = request.set("Accept", accept);
        }
        let response = request.call().map_err(|error| match error {
            ureq::Error::Status(code, _) => format!("HTTP {code} from {url}"),
            ureq::Error::Transport(transport) => transport.to_string(),
        })?;
        let mut body = Vec::new();
        response
            .into_reader()
            .take(MAX_TARBALL_BYTES as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|error| error.to_string())?;
        Ok(body)
    }
}

pub fn tarball_url(owner: &str, repo: &str, git_ref: &str) -> String {
    format!("https://codeload.github.com/{owner}/{repo}/tar.gz/{git_ref}")
}

pub fn commit_sha_url(owner: &str, repo: &str, git_ref: &str) -> String {
    format!("https://api.github.com/repos/{owner}/{repo}/commits/{git_ref}")
}

/// A repository snapshot: files keyed by `/`-separated path relative to the repo root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tarball {
    pub commit_sha: String,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl Tarball {
    /// `folder/`-prefixed keys, with the prefix removed. `folder` is `/`-separated, no trailing
    /// slash; the empty string means the whole tree.
    pub fn subtree(&self, folder: &str) -> BTreeMap<String, &[u8]> {
        if folder.is_empty() {
            return self
                .files
                .iter()
                .map(|(k, v)| (k.clone(), v.as_slice()))
                .collect();
        }
        let prefix = format!("{folder}/");
        self.files
            .range(prefix.clone()..)
            .take_while(|(k, _)| k.starts_with(&prefix))
            .map(|(k, v)| (k[prefix.len()..].to_string(), v.as_slice()))
            .collect()
    }

    pub fn text(&self, path: &str) -> Option<String> {
        self.files
            .get(path)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }
}

pub fn fetch_tarball(
    fetcher: &dyn Fetcher,
    owner: &str,
    repo: &str,
    git_ref: &str,
) -> Result<Tarball, AdapterError> {
    let url = tarball_url(owner, repo, git_ref);
    let bytes = fetcher
        .get(&url, None)
        .map_err(|error| AdapterError::message(format!("download failed: {error}")))?;
    unpack(&bytes)
}

pub fn upstream_sha(
    fetcher: &dyn Fetcher,
    owner: &str,
    repo: &str,
    git_ref: &str,
) -> Result<String, AdapterError> {
    let url = commit_sha_url(owner, repo, git_ref);
    let bytes = fetcher
        .get(&url, Some("application/vnd.github.sha"))
        .map_err(|error| AdapterError::message(format!("upstream check failed: {error}")))?;
    let text = String::from_utf8_lossy(&bytes);
    let sha = text.trim();
    if is_full_sha(sha) {
        Ok(sha.to_ascii_lowercase())
    } else {
        Err(AdapterError::message(format!(
            "upstream check for {owner}/{repo}@{git_ref} did not return a commit id"
        )))
    }
}

pub fn is_full_sha(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn unpack(bytes: &[u8]) -> Result<Tarball, AdapterError> {
    if bytes.len() > MAX_TARBALL_BYTES {
        return Err(AdapterError::message(format!(
            "download is too large (over {} MB)",
            MAX_TARBALL_BYTES / (1024 * 1024)
        )));
    }
    let mut archive = tar::Archive::new(GzDecoder::new(bytes));
    let mut commit_sha: Option<String> = None;
    let mut files = BTreeMap::new();
    let mut unpacked: u64 = 0;
    let entries = archive
        .entries()
        .map_err(|error| AdapterError::parse(format!("not a tar archive: {error}"), None))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|error| AdapterError::parse(format!("bad tar entry: {error}"), None))?;
        let entry_type = entry.header().entry_type();
        if entry_type == tar::EntryType::XGlobalHeader {
            let mut raw = Vec::new();
            entry
                .read_to_end(&mut raw)
                .map_err(|error| AdapterError::parse(error.to_string(), None))?;
            for record in tar::PaxExtensions::new(&raw).flatten() {
                if record.key() == Ok("comment") {
                    if let Ok(value) = record.value() {
                        let value = value.trim();
                        if is_full_sha(value) {
                            commit_sha = Some(value.to_ascii_lowercase());
                        }
                    }
                }
            }
            continue;
        }
        if !entry_type.is_file() {
            continue;
        }
        let raw_path = entry.path_bytes();
        let raw_path = String::from_utf8_lossy(&raw_path);
        let Some(relative) = strip_top_level(&raw_path)? else {
            continue;
        };
        let size = entry.header().size().unwrap_or(0);
        if size > MAX_FILE_BYTES {
            return Err(AdapterError::message(format!(
                "{relative} is too large to install"
            )));
        }
        unpacked += size;
        if unpacked > MAX_UNPACKED_BYTES {
            return Err(AdapterError::message(
                "repository is too large to unpack in memory",
            ));
        }
        let mut contents = Vec::with_capacity(size as usize);
        entry
            .read_to_end(&mut contents)
            .map_err(|error| AdapterError::parse(error.to_string(), Some(relative.clone())))?;
        files.insert(relative, contents);
    }
    let commit_sha = commit_sha.ok_or_else(|| {
        AdapterError::message("download did not carry a commit id (not a GitHub tarball?)")
    })?;
    Ok(Tarball { commit_sha, files })
}

/// Drops the `<repo>-<ref>/` folder GitHub wraps every tarball in and validates the rest.
/// `Ok(None)` for the folder entry itself; `Err` for anything that could escape a target dir.
fn strip_top_level(raw: &str) -> Result<Option<String>, AdapterError> {
    let mut parts = raw.split('/').filter(|part| !part.is_empty());
    let Some(top) = parts.next() else {
        return Ok(None);
    };
    if top == "." || top == ".." {
        return Err(traversal(raw));
    }
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        return Ok(None);
    }
    for part in &rest {
        if *part == ".." || *part == "." || part.contains('\\') || part.contains(':') {
            return Err(traversal(raw));
        }
    }
    Ok(Some(rest.join("/")))
}

fn traversal(raw: &str) -> AdapterError {
    AdapterError::parse(
        format!("archive entry escapes the repository root: {raw}"),
        None,
    )
}
