use super::*;
use std::fs;

mod memo;
mod providers;

fn catalog_versions(marketplace_root: &Path) -> HashMap<String, String> {
    catalog(marketplace_root)
        .into_iter()
        .filter(|(_, hint)| !hint.version.is_empty())
        .map(|(name, hint)| (name, hint.version))
        .collect()
}

#[test]
fn a_listed_marketplace_offers_the_first_version_it_lists_for_a_plugin() {
    let listing = Marketplace::listing(
        "acme",
        [
            ("tools".to_string(), Some("2.0.0".to_string())),
            ("tools".to_string(), Some("9.0.0".to_string())),
            ("kit".to_string(), None),
        ],
    );
    let hints = listing.offers.read(None);
    assert_eq!(
        hints.get("tools").map(|hint| hint.offered(None)),
        Some("2.0.0".into())
    );
    assert_eq!(
        hints.get("kit").map(|hint| hint.offered(None)),
        Some(String::new())
    );
}

#[test]
fn github_answers_with_the_file_or_its_absence_and_anything_else_is_a_failure() {
    let (url, server) = crate::http::serve_once("200 OK", r#"{"plugins":[]}"#);
    assert_eq!(fetch_text(&url), Ok(Some(r#"{"plugins":[]}"#.into())));
    server.join().unwrap();
    let (url, server) = crate::http::serve_once("404 Not Found", "404: Not Found");
    assert_eq!(fetch_text(&url), Ok(None));
    server.join().unwrap();
    let (url, server) = crate::http::serve_once("503 Service Unavailable", "busy");
    assert_eq!(fetch_text(&url), Err(Transient));
    server.join().unwrap();
    assert_eq!(fetch_text(&crate::http::refused_url()), Err(Transient));
}

#[test]
fn inventory_version_wins() {
    let root = crate::paths::scratch_dir("on-n-off-plugin-meta-inv").join("unknown");
    fs::create_dir_all(&root).unwrap();
    assert_eq!(installed(&root, Some("1.2.0")), "1.2.0");
    assert_eq!(installed(&root, Some(" unknown ")), "");
    fs::create_dir_all(root.join(".claude-plugin")).unwrap();
    fs::write(
        root.join(".claude-plugin/plugin.json"),
        r#"{"name":"workbench","version":"9.9.9"}"#,
    )
    .unwrap();
    assert_eq!(installed(&root, Some("1.2.0")), "1.2.0");
    assert_eq!(installed(&root, Some("unknown")), "9.9.9");
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
    assert_eq!(installed(&root, None), "0.22.1");
    fs::remove_file(root.join(".codex-plugin/plugin.json")).unwrap();
    assert_eq!(installed(&root, None), "0.22.1");
    let unknown = root.parent().unwrap().join("unknown");
    fs::create_dir_all(&unknown).unwrap();
    assert_eq!(installed(&unknown, Some("unknown")), "");
    assert_eq!(installed(&root.join("missing-cache"), None), "");
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
    let hints = catalog(&root);
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
    with_fetch_text(
        |url| {
            Ok((url
                == "https://raw.githubusercontent.com/obra/superpowers/b36e0829c6d0140e93cfef2ca599b1b07d4a7797/.claude-plugin/plugin.json")
                .then(|| r#"{"name":"superpowers","version":"6.3.0"}"#.into()))
        },
        || {
            let memo = Memo::new();
            let fetch = memo.reader(false);
            let hint = catalog(&root).remove("superpowers").unwrap();
            assert_eq!(hint.offered(None), "");
            let upstream = hint.offered(Some(&fetch));
            assert_eq!(upstream, "6.3.0");
            assert_eq!(compare("6.3.0", &upstream), ("6.3.0".into(), false));
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
    let mut hints = catalog(&root);
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
            Ok(Some(
                r#"{
                      "plugins": [
                        {"name":"workbench","version":"0.23.0","source":"./plugins/workbench"},
                        {"name":"toolkit","version":"0.6.0","source":"./plugins/toolkit"}
                      ]
                    }"#
                .into(),
            ))
        },
        || {
            let memo = Memo::new();
            merge_remote_marketplace(&mut hints, "example/workshop", &root, &memo.reader(false));
            assert_eq!(
                hints.get("workbench").map(|hint| hint.version.as_str()),
                Some("0.23.0")
            );
            assert_eq!(
                hints.get("toolkit").map(|hint| hint.version.as_str()),
                Some("0.6.0")
            );
            let upstream = hints.get("workbench").unwrap().offered(None);
            assert_eq!(upstream, "0.23.0");
            assert_eq!(compare("0.22.1", &upstream), ("0.23.0".into(), true));
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
    let sha_only = Hint {
        version: String::new(),
        pin: Some(Pin {
            url: "https://github.com/obra/superpowers.git".into(),
            path: String::new(),
            rev: "b36e0829c6d0140e93cfef2ca599b1b07d4a7797".into(),
        }),
    };
    let upstream = sha_only.offered(None);
    assert_eq!(upstream, "");
    assert_eq!(compare("6.3.0", &upstream), (String::new(), false));

    let behind = Hint {
        version: "0.23.0".into(),
        pin: None,
    };
    let upstream = behind.offered(None);
    assert_eq!(upstream, "0.23.0");
    assert_eq!(compare("0.22.1", &upstream), ("0.23.0".into(), true));
    assert_eq!(compare("v0.22.1", "0.22.1"), ("0.22.1".into(), false));
    assert_eq!(compare("1.0.0", ""), (String::new(), false));
    assert_eq!(compare("6.3.0", "b36e082"), (String::new(), false));
    assert_eq!(compare("b36e082", "6.3.0"), (String::new(), false));
    assert_eq!(compare("", "6.3.0"), (String::new(), false));
    assert_eq!(compare("1.0.0", " unknown "), (String::new(), false));
    assert_eq!(compare("1.0.0", " 2.0.0 "), ("2.0.0".into(), true));
}
