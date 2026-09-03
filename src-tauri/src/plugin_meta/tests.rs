use super::*;
use std::fs;

#[cfg(test)]
pub fn installed_version(install_path: &Path, inventory_version: Option<&str>) -> String {
    installed_hint(install_path, inventory_version).version
}

#[cfg(test)]
pub fn catalog_versions(marketplace_root: &Path) -> HashMap<String, String> {
    catalog_hints(marketplace_root)
        .into_iter()
        .filter(|(_, hint)| !hint.version.is_empty())
        .map(|(name, hint)| (name, hint.version))
        .collect()
}

#[test]
fn inventory_version_wins() {
    let root = crate::paths::scratch_dir("on-n-off-plugin-meta-inv").join("unknown");
    fs::create_dir_all(&root).unwrap();
    assert_eq!(installed_version(&root, Some("1.2.0")), "1.2.0");
    assert_eq!(installed_version(&root, Some(" unknown ")), "");
    let _ = fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn reads_codex_manifest_then_folder_name() {
    let root = crate::paths::scratch_dir("on-n-off-plugin-meta-dir").join("0.22.1");
    fs::create_dir_all(root.join(".codex-plugin")).unwrap();
    fs::write(
        root.join(".codex-plugin/plugin.json"),
        r#"{"name":"workbench","version":"0.22.1"}"#,
    )
    .unwrap();
    assert_eq!(installed_version(&root, None), "0.22.1");
    fs::remove_file(root.join(".codex-plugin/plugin.json")).unwrap();
    assert_eq!(installed_version(&root, None), "0.22.1");
    let unknown = root.parent().unwrap().join("unknown");
    fs::create_dir_all(&unknown).unwrap();
    assert_eq!(installed_version(&unknown, Some("unknown")), "");
    assert_eq!(installed_version(&root.join("missing-cache"), None), "");
    let _ = fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn catalog_uses_release_version_not_commit_sha() {
    let root = crate::paths::scratch_dir("on-n-off-plugin-meta-cat");
    fs::create_dir_all(root.join(".claude-plugin")).unwrap();
    fs::create_dir_all(root.join("plugins/workbench/.claude-plugin")).unwrap();
    fs::write(
        root.join("plugins/workbench/.claude-plugin/plugin.json"),
        r#"{"name":"workbench","version":"0.22.1"}"#,
    )
    .unwrap();
    fs::write(
        root.join(".claude-plugin/marketplace.json"),
        r#"{
              "plugins": [
                {"name":"workbench","version":"0.23.0","source":"./plugins/workbench"},
                {"name":"toolkit","source":{"source":"local","path":"./plugins/workbench"}},
                {"name":"api","source":{"source":"git-subdir","path":"plugins/api","ref":"v1.5.5"}},
                {"name":"mainline","source":{"source":"git-subdir","path":"plugins/mainline","ref":"main"}},
                {"name":"superpowers","source":{"source":"url","url":"https://github.com/obra/superpowers.git","sha":"b36e0829c6d0140e93cfef2ca599b1b07d4a7797"}}
              ]
            }"#,
    )
    .unwrap();
    let hints = catalog_hints(&root);
    assert_eq!(
        hints.get("workbench").map(|hint| hint.version.as_str()),
        Some("0.23.0")
    );
    assert_eq!(
        hints.get("toolkit").map(|hint| hint.version.as_str()),
        Some("0.22.1")
    );
    assert_eq!(
        hints.get("api").map(|hint| hint.version.as_str()),
        Some("v1.5.5")
    );
    assert!(!hints.contains_key("mainline"));
    assert_eq!(
        hints.get("superpowers").map(|hint| hint.version.as_str()),
        Some("")
    );
    with_remote_fetch(
        |url, path, rev| {
            assert!(url.contains("obra/superpowers"));
            assert!(path.is_empty());
            assert_eq!(rev, "b36e0829c6d0140e93cfef2ca599b1b07d4a7797");
            Some("6.3.0".into())
        },
        || {
            let mut hint = catalog_hints(&root).remove("superpowers").unwrap();
            fill_remote_version(&mut hint);
            assert_eq!(hint.version, "6.3.0");
            let installed = VersionHint {
                version: "6.3.0".into(),
                ..VersionHint::default()
            };
            let (version, upstream, drift) = resolve_versions(&installed, Some(&hint));
            assert_eq!(version, "6.3.0");
            assert_eq!(upstream, "6.3.0");
            assert!(!drift);
        },
    );
    let versions = catalog_versions(&root);
    assert_eq!(versions.get("api").map(String::as_str), Some("v1.5.5"));
    assert!(!versions.contains_key("superpowers"));
    assert_eq!(
        github_manifest_urls(
            "https://github.com/mongodb/agent-skills.git",
            "plugins/mongodb",
            "b4ea8150a020b9babaddc6c271c6dc177c06a83f",
        )[0],
        "https://raw.githubusercontent.com/mongodb/agent-skills/b4ea8150a020b9babaddc6c271c6dc177c06a83f/plugins/mongodb/.claude-plugin/plugin.json"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn remote_marketplace_json_beats_stale_local_clone() {
    let root = crate::paths::scratch_dir("on-n-off-plugin-meta-remote-mkt");
    fs::create_dir_all(root.join(".claude-plugin")).unwrap();
    fs::write(
        root.join(".claude-plugin/marketplace.json"),
        r#"{
              "plugins": [
                {"name":"workbench","version":"0.22.1","source":"./plugins/workbench"},
                {"name":"toolkit","version":"0.5.0","source":"./plugins/toolkit"}
              ]
            }"#,
    )
    .unwrap();
    let mut hints = catalog_hints(&root);
    assert_eq!(
        hints.get("workbench").map(|hint| hint.version.as_str()),
        Some("0.22.1")
    );
    assert_eq!(
        hints.get("toolkit").map(|hint| hint.version.as_str()),
        Some("0.5.0")
    );
    with_fetch_text(
        |url| {
            assert!(url.contains("example/workshop"));
            assert!(url.contains("marketplace.json"));
            Some(
                r#"{
                      "plugins": [
                        {"name":"workbench","version":"0.23.0","source":"./plugins/workbench"},
                        {"name":"toolkit","version":"0.6.0","source":"./plugins/toolkit"}
                      ]
                    }"#
                .into(),
            )
        },
        || {
            apply_remote_marketplace_versions(&mut hints, "example/workshop", &root);
            assert_eq!(
                hints.get("workbench").map(|hint| hint.version.as_str()),
                Some("0.23.0")
            );
            assert_eq!(
                hints.get("toolkit").map(|hint| hint.version.as_str()),
                Some("0.6.0")
            );
            let installed = VersionHint {
                version: "0.22.1".into(),
                ..VersionHint::default()
            };
            let (_, upstream, drift) = resolve_versions(&installed, hints.get("workbench"));
            assert_eq!(upstream, "0.23.0");
            assert!(drift);
        },
    );
    assert_eq!(
        github_repo("git@github.com:example/workshop.git"),
        Some(("example".into(), "workshop".into()))
    );
    assert_eq!(
        github_repo("example/workshop"),
        Some(("example".into(), "workshop".into()))
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolve_compares_release_versions_only() {
    let installed = VersionHint {
        version: "6.3.0".into(),
        ..VersionHint::default()
    };
    let sha_only = VersionHint {
        version: String::new(),
        remote_url: "https://github.com/obra/superpowers.git".into(),
        remote_rev: "b36e0829c6d0140e93cfef2ca599b1b07d4a7797".into(),
        ..VersionHint::default()
    };
    let (version, upstream, drift) = resolve_versions(&installed, Some(&sha_only));
    assert_eq!(version, "6.3.0");
    assert_eq!(upstream, "");
    assert!(!drift);

    let behind = VersionHint {
        version: "0.23.0".into(),
        ..VersionHint::default()
    };
    let installed_semver = VersionHint {
        version: "0.22.1".into(),
        ..VersionHint::default()
    };
    let (_, upstream, drift) = resolve_versions(&installed_semver, Some(&behind));
    assert_eq!(upstream, "0.23.0");
    assert!(drift);
    assert!(!out_of_sync("v0.22.1", "0.22.1"));
    assert!(!out_of_sync("1.0.0", ""));
    assert!(!out_of_sync("6.3.0", "b36e082"));
}

#[test]
fn strip_verbatim_drops_windows_prefix() {
    assert_eq!(
        strip_verbatim(r"\\?\C:\Users\me\.codex\.tmp\bundled"),
        PathBuf::from(r"C:\Users\me\.codex\.tmp\bundled")
    );
    assert_eq!(
        strip_verbatim(" /tmp/market "),
        PathBuf::from("/tmp/market")
    );
}
