//! Reads a marketplace snapshot: which plugins it lists and which skills/agents each carries.

use std::collections::HashMap;
use std::sync::Arc;

use super::deps::{self, EntryRef, SiblingIndex};
use super::fetch::Tarball;
use super::write::{self, normalize_upstream_path};
use crate::dto::{
    AdapterError, ItemKind, ItemSourceDto, MarketplaceEntryDto, MarketplaceInspectDto,
    MarketplacePluginDto,
};
use crate::plugin_meta::{
    parse_marketplace_text, parse_plugin_manifest, MarketplaceFile, PluginManifest,
    MARKETPLACE_MANIFESTS, PLUGIN_MANIFESTS,
};
use crate::scanner::parse_frontmatter;

pub const NOT_A_MARKETPLACE: &str =
    "No plugin marketplace manifest (.claude-plugin/marketplace.json) in this repository.";

/// Fetches the tarball of a plugin hosted in another repository: `(owner, repo, ref)`.
pub type SecondFetch<'a> =
    &'a mut dyn FnMut(&str, &str, Option<&str>) -> Result<Arc<Tarball>, AdapterError>;

/// The first marketplace manifest present in a snapshot.
pub fn marketplace_file(tarball: &Tarball) -> Option<MarketplaceFile> {
    MARKETPLACE_MANIFESTS
        .iter()
        .find_map(|relative| tarball.text(relative))
        .and_then(|text| parse_marketplace_text(&text))
}

/// Plugin name -> plugin folder inside the marketplace repository (`""` = repository root).
pub fn plugin_roots(tarball: &Tarball) -> HashMap<String, String> {
    let mut roots = HashMap::new();
    if let Some(manifest) = marketplace_file(tarball) {
        for plugin in &manifest.plugins {
            if let Some(local) = plugin.local_source_path() {
                if let Ok(root) = normalize_upstream_path(local) {
                    roots.insert(plugin.name.clone(), root);
                }
            }
        }
    }
    roots
}

/// A marketplace listing plus, per plugin, the snapshot its entries were read from (`None`
/// for unsupported plugins).
pub struct Inspected {
    pub dto: MarketplaceInspectDto,
    pub trees: Vec<Option<Arc<Tarball>>>,
}

/// The full listing: entries, plugin extras, and the dependencies between entries.
pub fn inspect(
    tarball: &Arc<Tarball>,
    fallback_name: &str,
    second: SecondFetch<'_>,
) -> MarketplaceInspectDto {
    let mut inspected = inspect_entries(tarball, fallback_name, second);
    annotate_dependencies(&mut inspected);
    inspected.dto
}

/// Entries and extras only; `annotate_dependencies` fills in the edges between them.
pub fn inspect_entries(
    tarball: &Arc<Tarball>,
    fallback_name: &str,
    second: SecondFetch<'_>,
) -> Inspected {
    let Some(manifest) = marketplace_file(tarball) else {
        return Inspected {
            dto: MarketplaceInspectDto {
                is_marketplace: false,
                commit_sha: tarball.commit_sha.clone(),
                marketplace_name: fallback_name.to_string(),
                plugins: Vec::new(),
                hint: Some(NOT_A_MARKETPLACE.to_string()),
            },
            trees: Vec::new(),
        };
    };
    let mut trees = Vec::with_capacity(manifest.plugins.len());
    let plugins = manifest
        .plugins
        .iter()
        .map(|plugin| {
            let base = MarketplacePluginDto {
                name: plugin.name.clone(),
                version: plugin.version.clone(),
                description: plugin.description.clone().unwrap_or_default(),
                supported: false,
                source: None,
                skills: Vec::new(),
                agents: Vec::new(),
                extras: Vec::new(),
            };
            if let Some(local) = plugin.local_source_path() {
                let Ok(root) = normalize_upstream_path(local) else {
                    trees.push(None);
                    return base;
                };
                trees.push(Some(Arc::clone(tarball)));
                return describe(tarball, &root, base);
            }
            if let Some((owner, repo, git_ref)) = plugin.github_source() {
                let Ok(remote) = second(&owner, &repo, git_ref.as_deref()) else {
                    trees.push(None);
                    return base;
                };
                let source = ItemSourceDto {
                    owner,
                    repo,
                    git_ref: git_ref.unwrap_or_else(|| "HEAD".to_string()),
                };
                let described = describe(
                    &remote,
                    "",
                    MarketplacePluginDto {
                        source: Some(source),
                        ..base
                    },
                );
                trees.push(Some(remote));
                return described;
            }
            trees.push(None);
            base
        })
        .collect();
    Inspected {
        dto: MarketplaceInspectDto {
            is_marketplace: true,
            commit_sha: tarball.commit_sha.clone(),
            marketplace_name: manifest
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| fallback_name.to_string()),
            plugins,
            hint: None,
        },
        trees,
    }
}

/// Runs the dependency scanner over every entry of every supported plugin. Siblings are
/// indexed marketplace-wide, so an entry can depend on one in another plugin.
pub fn annotate_dependencies(inspected: &mut Inspected) {
    let index = SiblingIndex::from_inspect(&inspected.dto);
    for (plugin, tree) in inspected.dto.plugins.iter_mut().zip(&inspected.trees) {
        let Some(tree) = tree else {
            continue;
        };
        let plugin_name = plugin.name.clone();
        for (kind, list) in [
            (ItemKind::Skill, &mut plugin.skills),
            (ItemKind::Agent, &mut plugin.agents),
        ] {
            for entry in list.iter_mut() {
                let me = EntryRef {
                    plugin_name: plugin_name.clone(),
                    kind,
                    path: entry.path.clone(),
                    name: entry.name.clone(),
                };
                let Ok(files) = write::item_files(tree, &entry.path, kind) else {
                    continue;
                };
                let found = deps::detect(&files, &me, &index);
                entry.depends_on = found.depends_on;
                entry.external_refs = found.external_refs;
                entry.uses_plugin_root = found.uses_plugin_root;
            }
        }
    }
}

/// Reads one plugin folder out of a snapshot: its manifest, skills, and agents.
pub fn describe(tree: &Tarball, root: &str, base: MarketplacePluginDto) -> MarketplacePluginDto {
    let manifest = plugin_manifest(tree, root);
    let version = manifest
        .as_ref()
        .and_then(|m| m.version.clone())
        .or(base.version);
    let description = manifest
        .as_ref()
        .and_then(|m| m.description.clone())
        .filter(|d| !d.trim().is_empty())
        .unwrap_or(base.description);
    let skills = match manifest.as_ref().and_then(|m| m.skills.as_ref()) {
        Some(listed) if !listed.is_empty() => listed_skills(tree, root, listed),
        _ => scanned_skills(tree, root, &base.name),
    };
    MarketplacePluginDto {
        version,
        description,
        supported: true,
        skills,
        agents: agents(tree, root),
        extras: extras(tree, root, manifest.as_ref()),
        ..base
    }
}

/// Plugin-level assets a local skill/agent copy never gets: `commands`, `hooks`, `mcp`.
fn extras(tree: &Tarball, root: &str, manifest: Option<&PluginManifest>) -> Vec<String> {
    let has_folder = |folder: &str| {
        let prefix = format!("{}/", join(root, folder));
        tree.files
            .range(prefix.clone()..)
            .next()
            .is_some_and(|(key, _)| key.starts_with(&prefix))
    };
    let declared = |value: Option<&serde_json::Value>| {
        value.is_some_and(|value| match value {
            serde_json::Value::Null => false,
            serde_json::Value::Array(items) => !items.is_empty(),
            serde_json::Value::Object(map) => !map.is_empty(),
            serde_json::Value::String(text) => !text.trim().is_empty(),
            _ => true,
        })
    };
    let mut out = Vec::new();
    if has_folder("commands") || declared(manifest.and_then(|m| m.commands.as_ref())) {
        out.push("commands".to_string());
    }
    if has_folder("hooks") || declared(manifest.and_then(|m| m.hooks.as_ref())) {
        out.push("hooks".to_string());
    }
    if tree.text(&join(root, ".mcp.json")).is_some()
        || declared(manifest.and_then(|m| m.mcp_servers.as_ref()))
    {
        out.push("mcp".to_string());
    }
    out
}

pub fn plugin_manifest(tree: &Tarball, root: &str) -> Option<PluginManifest> {
    PLUGIN_MANIFESTS
        .iter()
        .find_map(|relative| tree.text(&join(root, relative)))
        .and_then(|text| parse_plugin_manifest(&text))
}

fn listed_skills(tree: &Tarball, root: &str, listed: &[String]) -> Vec<MarketplaceEntryDto> {
    let mut out = Vec::new();
    for entry in listed {
        let Ok(relative) = normalize_upstream_path(entry) else {
            continue;
        };
        let folder = match relative.strip_suffix("/SKILL.md") {
            Some(parent) => parent.to_string(),
            None if relative == "SKILL.md" => String::new(),
            None => relative,
        };
        let path = join(root, &folder);
        if let Some(item) = skill_entry(tree, &path) {
            if !out
                .iter()
                .any(|existing: &MarketplaceEntryDto| existing.path == item.path)
            {
                out.push(item);
            }
        }
    }
    out
}

fn scanned_skills(tree: &Tarball, root: &str, plugin_name: &str) -> Vec<MarketplaceEntryDto> {
    let mut out = Vec::new();
    let skills_dir = join(root, "skills");
    for folder in child_folders_with(tree, &skills_dir, "SKILL.md") {
        if let Some(item) = skill_entry(tree, &join(&skills_dir, &folder)) {
            out.push(item);
        }
    }
    if out.is_empty() {
        if let Some(contents) = tree.text(&join(root, "SKILL.md")) {
            let (name, description) = parse_frontmatter(&contents, plugin_name);
            out.push(MarketplaceEntryDto {
                name,
                description,
                path: root.to_string(),
                ..MarketplaceEntryDto::default()
            });
        }
    }
    out
}

fn skill_entry(tree: &Tarball, folder: &str) -> Option<MarketplaceEntryDto> {
    let contents = tree.text(&join(folder, "SKILL.md"))?;
    let fallback = folder.rsplit('/').next().unwrap_or(folder);
    let (name, description) = parse_frontmatter(&contents, fallback);
    Some(MarketplaceEntryDto {
        name,
        description,
        path: folder.to_string(),
        ..MarketplaceEntryDto::default()
    })
}

fn agents(tree: &Tarball, root: &str) -> Vec<MarketplaceEntryDto> {
    let agents_dir = join(root, "agents");
    let prefix = format!("{agents_dir}/");
    tree.files
        .range(prefix.clone()..)
        .take_while(|(k, _)| k.starts_with(&prefix))
        .filter_map(|(key, bytes)| {
            let file = &key[prefix.len()..];
            if file.contains('/') || !file.ends_with(".md") {
                return None;
            }
            let stem = file.trim_end_matches(".md");
            let contents = String::from_utf8_lossy(bytes);
            let (name, description) = parse_frontmatter(&contents, stem);
            Some(MarketplaceEntryDto {
                name,
                description,
                path: key.clone(),
                ..MarketplaceEntryDto::default()
            })
        })
        .collect()
}

/// Direct child folders of `dir` that contain `marker`, in name order.
fn child_folders_with(tree: &Tarball, dir: &str, marker: &str) -> Vec<String> {
    let prefix = format!("{dir}/");
    let suffix = format!("/{marker}");
    tree.files
        .range(prefix.clone()..)
        .take_while(|(k, _)| k.starts_with(&prefix))
        .filter_map(|(key, _)| {
            let rest = &key[prefix.len()..];
            let folder = rest.strip_suffix(suffix.as_str())?;
            (!folder.is_empty() && !folder.contains('/')).then(|| folder.to_string())
        })
        .collect()
}

pub fn join(root: &str, relative: &str) -> String {
    match (root.is_empty(), relative.is_empty()) {
        (true, _) => relative.to_string(),
        (false, true) => root.to_string(),
        (false, false) => format!("{root}/{relative}"),
    }
}
