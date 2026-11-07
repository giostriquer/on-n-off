use std::fs;
use std::path::{Path, PathBuf};

use crate::adapter::{scratch_adapter, AgentAdapter};
use crate::dto::{AgentId, AgentTabDto};

use super::super::{read, with_fetch_text, Memo, Reach, Transient};

pub(super) const WORKSHOP_MARKETPLACE: &str =
    "https://raw.githubusercontent.com/acme/workshop/main/.claude-plugin/marketplace.json";
pub(super) const SUPERPOWERS_MANIFEST: &str = "https://raw.githubusercontent.com/acme/drift-superpowers/b36e0829c6d0140e93cfef2ca599b1b07d4a7797/.claude-plugin/plugin.json";

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

pub(super) fn remote(url: &str) -> Result<Option<String>, Transient> {
    Ok(match url {
        WORKSHOP_MARKETPLACE => Some(
            r#"{"plugins":[{"name":"workbench","version":"2.0.0"},{"name":"toolkit","version":"0.5.0"}]}"#
                .into(),
        ),
        SUPERPOWERS_MANIFEST => Some(r#"{"name":"superpowers","version":"6.3.0"}"#.into()),
        _ => None,
    })
}

fn seed_claude(home: &Path) {
    let root = home.join(".claude");
    let marketplace = root.join("plugins/marketplaces/workshop");
    write(
        &marketplace.join(".claude-plugin/marketplace.json"),
        r#"{"plugins":[
            {"name":"workbench","version":"1.1.0","source":"./plugins/workbench"},
            {"name":"toolkit","source":"./plugins/toolkit"},
            {"name":"superpowers","source":{"source":"url","url":"https://github.com/acme/drift-superpowers.git","sha":"b36e0829c6d0140e93cfef2ca599b1b07d4a7797"}},
            {"name":"quiet","version":"1.0.0"},
            {"name":"blank","version":"3.0.0"}
        ]}"#,
    );
    write(
        &marketplace.join("plugins/toolkit/.claude-plugin/plugin.json"),
        r#"{"name":"toolkit","version":"0.5.0"}"#,
    );
    write(
        &root.join("plugins/known_marketplaces.json"),
        &serde_json::json!({ "workshop": {
            "source": { "repo": "acme/workshop" },
            "installLocation": marketplace,
        } })
        .to_string(),
    );
    let cache = root.join("plugins/cache");
    let install = |plugin: &str, folder: &str| {
        let path = cache.join("workshop").join(plugin).join(folder);
        fs::create_dir_all(&path).unwrap();
        path
    };
    let toolkit = install("toolkit", "0123456789ab");
    write(
        &toolkit.join(".claude-plugin/plugin.json"),
        r#"{"name":"toolkit","version":"0.5.0"}"#,
    );
    let entry = |path: PathBuf, version: Option<&str>| {
        let mut entry = serde_json::json!({ "scope": "user", "installPath": path });
        if let Some(version) = version {
            entry["version"] = version.into();
        }
        serde_json::json!([entry])
    };
    write(
        &root.join("plugins/installed_plugins.json"),
        &serde_json::json!({ "version": 2, "plugins": {
            "workbench@workshop": entry(install("workbench", "1.0.0"), Some("1.0.0")),
            "toolkit@workshop": entry(toolkit, None),
            "superpowers@workshop": entry(install("superpowers", "6.2.0"), Some("6.2.0")),
            "quiet@workshop": entry(install("quiet", "a1b2c3d4e5f6"), Some("a1b2c3d4e5f6")),
            "blank@workshop": entry(install("blank", "fedcba987654"), Some("unknown")),
            "solo@elsewhere": entry(install("solo", "1.0.0"), Some("1.0.0")),
        } })
        .to_string(),
    );
    write(&root.join("settings.json"), "{}");
}

fn seed_codex(home: &Path) {
    let root = home.join(".codex");
    write(
        &root.join("plugins/cache/workshop/workbench/0.22.1/.codex-plugin/plugin.json"),
        r#"{"name":"workbench","version":"0.22.1"}"#,
    );
    fs::create_dir_all(root.join("plugins/cache/workshop/toolkit/0.5.0")).unwrap();
    write(
        &root.join(".tmp/marketplaces/workshop/.claude-plugin/marketplace.json"),
        r#"{"plugins":[{"name":"workbench","version":"0.23.0"},{"name":"toolkit","version":"0.5.0"}]}"#,
    );
    write(
        &root.join("config.toml"),
        "[marketplaces.workshop]\nsource_type = \"git\"\nsource = \"https://github.com/acme/workshop.git\"\n\n[plugins.\"workbench@workshop\"]\nenabled = true\n\n[plugins.\"toolkit@workshop\"]\nenabled = true\n\n[plugins.\"quiet@elsewhere\"]\nenabled = false\n",
    );
}

fn seed_antigravity(home: &Path) {
    let gemini = home.join(".gemini");
    write(
        &gemini.join("antigravity-cli/plugins/alpha/plugin.json"),
        r#"{"name":"alpha","version":"1.0.0"}"#,
    );
    write(
        &gemini.join("config/plugins/beta/plugin.json"),
        r#"{"name":"beta"}"#,
    );
}

fn seed_cursor(home: &Path) {
    write(
        &home.join(".cursor/plugins/local/acme/plugin.json"),
        r#"{"name":"acme","version":"2.1.0"}"#,
    );
}

fn seed_muse(home: &Path) {
    let data = crate::paths::muse_data_for(home);
    let record = |id: &str, version: &str| {
        let package = data.join(format!("plugins/cache/local/{id}/abc/package"));
        write(
            &package.join(".muse-plugin/plugin.json"),
            &format!(r#"{{"name":"{id}","version":"{version}"}}"#),
        );
        serde_json::json!({
            "id": id,
            "version": version,
            "enabled": true,
            "source": { "path": format!("/srv/acme/{id}") },
            "cache_path": package,
        })
    };
    write(
        &data.join("plugins/installed.json"),
        &serde_json::json!({ "version": 1, "plugins": {
            "acme-tools": record("acme-tools", "1.0.0"),
            "quiet": record("quiet", "1.0.0"),
            "solo": record("solo", "1.0.0"),
        } })
        .to_string(),
    );
    write(
        &data.join("plugins/marketplaces/acme/snapshot.json"),
        r#"{"plugins":[{"name":"acme-tools","version":" 2.0.0 "},{"name":"quiet","version":"b36e0829c6d0"},{"name":"solo","version":"unknown"}]}"#,
    );
    write(
        &data.join("plugins/marketplaces.json"),
        r#"{"marketplaces":{"acme":{"source":{"path":"/srv/acme"},"snapshot_path":"plugins/marketplaces/acme/snapshot.json"}}}"#,
    );
}

pub(super) struct Home {
    home: PathBuf,
    pub(super) adapter: Box<dyn AgentAdapter>,
}

impl Home {
    pub(super) fn new(id: AgentId) -> Self {
        let home = crate::paths::scratch_dir(&format!("on-n-off-drift-{}", id.key()));
        match id {
            AgentId::Claude => seed_claude(&home),
            AgentId::Codex => seed_codex(&home),
            AgentId::Antigravity => seed_antigravity(&home),
            AgentId::Cursor => seed_cursor(&home),
            AgentId::Muse => seed_muse(&home),
        }
        let adapter = scratch_adapter(id, &home);
        Self { home, adapter }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

type Row = (&'static str, &'static str, &'static str, bool);

pub(super) fn rows(tab: &AgentTabDto) -> Vec<(String, String, String, bool)> {
    let mut rows: Vec<_> = tab
        .plugins
        .iter()
        .map(|plugin| {
            (
                plugin.id.clone(),
                plugin.version.clone(),
                plugin.upstream.clone(),
                plugin.out_of_sync,
            )
        })
        .collect();
    rows.sort();
    rows
}

fn owned(rows: &[Row]) -> Vec<(String, String, String, bool)> {
    rows.iter()
        .map(|(id, version, upstream, out)| {
            (
                id.to_string(),
                version.to_string(),
                upstream.to_string(),
                *out,
            )
        })
        .collect()
}

fn local_rows(id: AgentId) -> Vec<Row> {
    match id {
        AgentId::Claude => vec![
            ("blank@workshop", "", "", false),
            ("quiet@workshop", "a1b2c3d4e5f6", "", false),
            ("solo@elsewhere", "1.0.0", "", false),
            ("superpowers@workshop", "6.2.0", "", false),
            ("toolkit@workshop", "0.5.0", "0.5.0", false),
            ("workbench@workshop", "1.0.0", "1.1.0", true),
        ],
        AgentId::Codex => vec![
            ("quiet@elsewhere", "", "", false),
            ("toolkit@workshop", "0.5.0", "0.5.0", false),
            ("workbench@workshop", "0.22.1", "0.23.0", true),
        ],
        AgentId::Antigravity => vec![("alpha@cli", "", "", false), ("beta@config", "", "", false)],
        AgentId::Cursor => vec![("acme@local", "2.1.0", "", false)],
        AgentId::Muse => vec![
            ("acme-tools@acme", "1.0.0", "2.0.0", true),
            ("quiet@acme", "1.0.0", "", false),
            ("solo@acme", "1.0.0", "", false),
        ],
    }
}

fn remote_rows(id: AgentId) -> Vec<Row> {
    match id {
        AgentId::Claude => vec![
            ("blank@workshop", "", "", false),
            ("quiet@workshop", "a1b2c3d4e5f6", "", false),
            ("solo@elsewhere", "1.0.0", "", false),
            ("superpowers@workshop", "6.2.0", "6.3.0", true),
            ("toolkit@workshop", "0.5.0", "0.5.0", false),
            ("workbench@workshop", "1.0.0", "2.0.0", true),
        ],
        AgentId::Codex => vec![
            ("quiet@elsewhere", "", "", false),
            ("toolkit@workshop", "0.5.0", "0.5.0", false),
            ("workbench@workshop", "0.22.1", "2.0.0", true),
        ],
        AgentId::Antigravity | AgentId::Cursor | AgentId::Muse => local_rows(id),
    }
}

type Answers = Vec<(AgentId, Vec<(String, String, String, bool)>)>;

fn every_provider(read_one: impl Fn(&Home) -> AgentTabDto, expected: fn(AgentId) -> Vec<Row>) {
    let mut answered: Answers = Vec::new();
    let mut wanted: Answers = Vec::new();
    for id in AgentId::ALL {
        let home = Home::new(id);
        answered.push((id, rows(&read_one(&home))));
        wanted.push((id, owned(&expected(id))));
    }
    assert_eq!(answered, wanted);
}

#[test]
fn each_provider_answers_what_is_installed_upstream_and_out_of_sync_locally() {
    every_provider(
        |home| {
            with_fetch_text(
                |url| panic!("a local read fetched {url}"),
                || read(&*home.adapter, None, Reach::Local).unwrap(),
            )
        },
        local_rows,
    );
}

#[test]
fn each_provider_answers_what_is_installed_upstream_and_out_of_sync_remotely() {
    every_provider(
        |home| {
            let memo = Memo::new();
            with_fetch_text(remote, || {
                read(&*home.adapter, None, Reach::Remote(&memo)).unwrap()
            })
        },
        remote_rows,
    );
}
