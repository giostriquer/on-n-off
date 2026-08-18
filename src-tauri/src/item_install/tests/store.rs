use super::*;

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
    files.insert("ref/a.md".to_string(), b"a".to_vec());
    let expected = write::hash_files(&files);
    assert!(expected.contains_key("ref/a.md"));
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
        vec![
            "SKILL.md".to_string(),
            "ref/notes.md".to_string(),
            "reference.md".to_string()
        ]
    );
    let agent = write::item_files(&tarball, "agents/reviewer.md", ItemKind::Agent).unwrap();
    assert_eq!(
        agent.keys().cloned().collect::<Vec<_>>(),
        vec!["reviewer.md".to_string()]
    );
    let win = write::item_files(&tarball, r"skills\engineering\tdd", ItemKind::Skill).unwrap();
    assert_eq!(win.len(), 3);
    assert!(write::item_files(&tarball, "../x", ItemKind::Skill).is_err());
    assert!(write::item_files(&tarball, "skills/nope", ItemKind::Skill).is_err());
}
