//! Heuristic dependency detection between marketplace entries.
//!
//! Skills name the sibling skills they drive only in prose (`/deploy`, `` `lint` ``,
//! `Skill(review)`); there is no dependency field in `SKILL.md` or `plugin.json`. This
//! module scans every text file of an entry for the names of the other entries in the
//! marketplace and grades each hit:
//!
//! - **high**: the name is used like a command or an identifier — `` `/N` ``, `` `N` ``,
//!   `/N` in prose, `Skill(N)`, `skill: N`, `--skill N`, or a path into the sibling's folder
//!   (`skills/…/N/`, `../N/`).
//! - **medium**: the name appears in a phrase — `N skill`, `the N`, `run N`, `use N`.
//!
//! It also notes what a local copy of the entry will not carry: relative paths that leave the
//! item (`../lib/x`), paths into other `skills/` folders that are not siblings, and any use of
//! `CLAUDE_PLUGIN_ROOT`. Nothing here is authoritative; the picker only auto-adds high
//! confidence hits and the user can always uncheck them.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::OnceLock;

use regex::Regex;

use super::write::ItemFiles;
use crate::dto::{DepConfidence, ItemDependencyDto, ItemKind, MarketplaceInspectDto};

/// Names shorter than this are too common to trust (`go`, `ai`).
const MIN_NAME_LEN: usize = 3;
/// Files past this size are not prose; skip them rather than scan them.
const MAX_TEXT_BYTES: usize = 1024 * 1024;

/// One installable entry of a marketplace, as the scanner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRef {
    pub plugin_name: String,
    pub kind: ItemKind,
    pub path: String,
    pub name: String,
}

impl EntryRef {
    /// `skills/ops/deploy` -> `deploy`; `agents/auditor.md` -> `auditor`.
    fn folder_name(&self) -> &str {
        let last = self.path.rsplit('/').next().unwrap_or(&self.path);
        match self.kind {
            ItemKind::Skill => last,
            ItemKind::Agent => last.strip_suffix(".md").unwrap_or(last),
        }
    }
}

struct Matcher {
    name: String,
    high: Regex,
    medium: Regex,
    /// Indices into `SiblingIndex::entries`, in marketplace order.
    entries: Vec<usize>,
}

/// Every entry of a marketplace plus one compiled matcher per distinct name.
pub struct SiblingIndex {
    entries: Vec<EntryRef>,
    names: BTreeSet<String>,
    matchers: Vec<Matcher>,
}

impl SiblingIndex {
    /// Every entry of every supported plugin, in display order.
    pub fn from_inspect(dto: &MarketplaceInspectDto) -> Self {
        let mut entries = Vec::new();
        for plugin in dto.plugins.iter().filter(|plugin| plugin.supported) {
            for (kind, list) in [
                (ItemKind::Skill, &plugin.skills),
                (ItemKind::Agent, &plugin.agents),
            ] {
                for entry in list {
                    entries.push(EntryRef {
                        plugin_name: plugin.name.clone(),
                        kind,
                        path: entry.path.clone(),
                        name: entry.name.clone(),
                    });
                }
            }
        }
        Self::from_entries(entries)
    }

    pub fn from_entries(entries: Vec<EntryRef>) -> Self {
        let mut by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, entry) in entries.iter().enumerate() {
            let mut names = vec![entry.name.trim().to_string()];
            let folder = entry.folder_name().trim().to_string();
            if !names.contains(&folder) {
                names.push(folder);
            }
            for name in names {
                if name.chars().count() < MIN_NAME_LEN || name.contains(char::is_whitespace) {
                    continue;
                }
                let slot = by_name.entry(name).or_default();
                if !slot.contains(&index) {
                    slot.push(index);
                }
            }
        }
        let names: BTreeSet<String> = by_name.keys().cloned().collect();
        let matchers = by_name
            .into_iter()
            .filter_map(|(name, entries)| {
                let escaped = regex::escape(&name);
                Some(Matcher {
                    high: Regex::new(&high_pattern(&escaped)).ok()?,
                    medium: Regex::new(&medium_pattern(&escaped)).ok()?,
                    name,
                    entries,
                })
            })
            .collect();
        Self {
            entries,
            names,
            matchers,
        }
    }

    /// The entry the marketplace lists for `(plugin, kind, path)`, if any.
    pub fn find(&self, plugin_name: &str, kind: ItemKind, path: &str) -> Option<&EntryRef> {
        self.entries.iter().find(|entry| {
            entry.plugin_name == plugin_name && entry.kind == kind && entry.path == path
        })
    }
}

/// `N` used like a command or identifier. No look-around in the `regex` crate, so the
/// surrounding characters are matched explicitly; `(?m)` lets `^`/`$` see line ends.
fn high_pattern(name: &str) -> String {
    let after = r"(?:[^\w-]|$)";
    let forms = [
        format!("`/{name}`"),
        format!("`{name}`"),
        format!(r"(?:^|[^\w/.-])/{name}(?:[^\w/-]|$)"),
        format!(r#"Skill\(["']?{name}["']?\)"#),
        // `call the Skill tool with "lint" and "review"`: a quoted name shortly after "skill".
        format!(r#"(?i:skill)[^\n]{{0,60}}["']{name}["']"#),
        format!(r"(?i:skill):[ \t]*{name}{after}"),
        format!(r"--skill[ \t]+{name}{after}"),
        format!(r"skills/(?:[\w.-]+/)*{name}/"),
        format!(r"\.\./{name}/"),
    ];
    format!("(?m){}", forms.join("|"))
}

/// `N` inside a phrase that usually means "that skill".
fn medium_pattern(name: &str) -> String {
    format!(
        "(?m)(?:^|[^\\w-]){name}(?i: skill)(?:[^\\w-]|$)|(?:^|[^\\w-])(?i:the|run|use) {name}(?:[^\\w-]|$)"
    )
}

fn external_ref_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(\.\./[\w.-]+(?:/[\w.-]+)*)|(?:^|[^\w/.-])(skills/[\w.-]+(?:/[\w.-]+)*)")
            .expect("static regex")
    })
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DetectedDeps {
    pub depends_on: Vec<ItemDependencyDto>,
    pub external_refs: Vec<String>,
    pub uses_plugin_root: bool,
}

/// Scans the text files of `me` for the other entries in `siblings`.
pub fn detect(files: &ItemFiles, me: &EntryRef, siblings: &SiblingIndex) -> DetectedDeps {
    let mut best: HashMap<usize, DepConfidence> = HashMap::new();
    let mut external: BTreeSet<String> = BTreeSet::new();
    let mut uses_plugin_root = false;
    for bytes in files.values() {
        if bytes.len() > MAX_TEXT_BYTES {
            continue;
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            continue;
        };
        if text.contains("CLAUDE_PLUGIN_ROOT") {
            uses_plugin_root = true;
        }
        for caps in external_ref_regex().captures_iter(text) {
            let Some(found) = caps.get(1).or_else(|| caps.get(2)) else {
                continue;
            };
            let path = found
                .as_str()
                .trim_end_matches(['.', ',', ';', ':', ')', '\'', '"']);
            if is_external(path, me, siblings) {
                external.insert(path.to_string());
            }
        }
        for matcher in &siblings.matchers {
            if !text.contains(matcher.name.as_str()) {
                continue;
            }
            let confidence = if matcher.high.is_match(text) {
                DepConfidence::High
            } else if matcher.medium.is_match(text) {
                DepConfidence::Medium
            } else {
                continue;
            };
            for index in resolve(matcher, me, siblings) {
                let slot = best.entry(index).or_insert(confidence);
                if confidence > *slot {
                    *slot = confidence;
                }
            }
        }
    }
    let mut depends_on: Vec<ItemDependencyDto> = best
        .into_iter()
        .map(|(index, confidence)| {
            let entry = &siblings.entries[index];
            ItemDependencyDto {
                plugin_name: entry.plugin_name.clone(),
                kind: entry.kind,
                path: entry.path.clone(),
                name: entry.name.clone(),
                confidence,
            }
        })
        .collect();
    depends_on
        .sort_by(|a, b| (&a.plugin_name, a.kind, &a.path).cmp(&(&b.plugin_name, b.kind, &b.path)));
    DetectedDeps {
        depends_on,
        external_refs: external.into_iter().collect(),
        uses_plugin_root,
    }
}

/// Which entries a matched name stands for: never `me`; a sibling in the same plugin beats a
/// same-named entry elsewhere, and outside the plugin only the first (marketplace order) counts.
fn resolve(matcher: &Matcher, me: &EntryRef, siblings: &SiblingIndex) -> Vec<usize> {
    let candidates: Vec<usize> = matcher
        .entries
        .iter()
        .copied()
        .filter(|&index| &siblings.entries[index] != me)
        .collect();
    let same_plugin: Vec<usize> = candidates
        .iter()
        .copied()
        .filter(|&index| siblings.entries[index].plugin_name == me.plugin_name)
        .collect();
    if !same_plugin.is_empty() {
        return same_plugin;
    }
    candidates.into_iter().take(1).collect()
}

/// A path reference counts as external when it leaves the item and does not point into a
/// sibling entry (those are dependencies, reported separately).
fn is_external(path: &str, me: &EntryRef, siblings: &SiblingIndex) -> bool {
    let own_prefix = format!("{}/", me.path);
    if path == me.path || path.starts_with(&own_prefix) {
        return false;
    }
    !path
        .split('/')
        .any(|segment| segment == me.folder_name() || siblings.names.contains(segment))
}
