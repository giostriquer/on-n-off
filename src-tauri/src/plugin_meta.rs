use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub(crate) struct PluginManifest {
    #[serde(default)]
    pub(crate) version: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) skills: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) commands: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) hooks: Option<serde_json::Value>,
    #[serde(default, rename = "mcpServers")]
    pub(crate) mcp_servers: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MarketplaceFile {
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) plugins: Vec<MarketplacePlugin>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MarketplacePlugin {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) version: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) source: Option<MarketplaceSource>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum MarketplaceSource {
    Path(String),
    Object {
        #[serde(default)]
        source: Option<String>,
        #[serde(default)]
        repo: Option<String>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        #[serde(rename = "ref")]
        git_ref: Option<String>,
        #[serde(default)]
        sha: Option<String>,
    },
}

pub(crate) const MARKETPLACE_MANIFESTS: &[&str] = &[
    ".claude-plugin/marketplace.json",
    ".codex-plugin/marketplace.json",
    ".agents/plugins/marketplace.json",
    ".cursor-plugin/marketplace.json",
];

pub(crate) const PLUGIN_MANIFESTS: &[&str] = &[
    ".cursor-plugin/plugin.json",
    ".codex-plugin/plugin.json",
    ".claude-plugin/plugin.json",
    ".muse-plugin/plugin.json",
    "plugin.json",
];

pub(crate) fn parse_marketplace_text(text: &str) -> Option<MarketplaceFile> {
    serde_json::from_str::<MarketplaceFile>(text).ok()
}

pub(crate) fn parse_plugin_manifest(text: &str) -> Option<PluginManifest> {
    serde_json::from_str::<PluginManifest>(text).ok()
}

pub fn strip_verbatim(path: &str) -> PathBuf {
    let path = path.trim();
    PathBuf::from(path.strip_prefix(r"\\?\").unwrap_or(path))
}

impl MarketplacePlugin {
    pub(crate) fn local_source_path(&self) -> Option<&str> {
        match &self.source {
            Some(MarketplaceSource::Path(path)) => Some(path.as_str()),
            Some(MarketplaceSource::Object {
                path, url, source, ..
            }) if url.as_deref().unwrap_or("").is_empty()
                && source
                    .as_deref()
                    .is_none_or(|kind| kind.is_empty() || kind == "local") =>
            {
                path.as_deref()
            }
            _ => None,
        }
    }

    pub(crate) fn github_source(&self) -> Option<(String, String, Option<String>)> {
        let Some(MarketplaceSource::Object {
            source,
            repo,
            git_ref,
            ..
        }) = &self.source
        else {
            return None;
        };
        if source.as_deref() != Some("github") {
            return None;
        }
        let repo = repo.as_deref()?.trim().trim_end_matches(".git");
        let (owner, name) = repo.split_once('/')?;
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            return None;
        }
        Some((owner.to_string(), name.to_string(), git_ref.clone()))
    }
}

#[cfg(test)]
mod tests;
