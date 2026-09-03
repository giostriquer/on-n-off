use super::*;

#[test]
fn backup_copies_file_and_prunes_to_last_twenty() {
    let root = crate::paths::scratch_dir("on-n-off-backup");
    let file = root.join("settings.json");
    fs::write(&file, "{\"a\":1}").unwrap();
    let store = BackupStore::at(root.join("backups"));
    for i in 0..22 {
        fs::write(&file, format!("{{\"a\":{i}}}")).unwrap();
        store.backup(AgentId::Claude, &file).unwrap();
    }
    let dir = root.join("backups/claude");
    let count = fs::read_dir(&dir).unwrap().count();
    assert_eq!(count, 20);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn backup_item_copies_folders_and_prunes() {
    let root = crate::paths::scratch_dir("on-n-off-backup-item");
    let skill = root.join("tdd");
    fs::create_dir_all(skill.join("ref")).unwrap();
    fs::write(skill.join("SKILL.md"), "v").unwrap();
    fs::write(skill.join("ref/a.md"), "a").unwrap();
    let store = BackupStore::at(root.join("backups"));
    let mut last = None;
    for _ in 0..22 {
        last = store.backup_item(AgentId::Codex, &skill).unwrap();
    }
    let copy = last.unwrap();
    assert_eq!(fs::read_to_string(copy.join("ref/a.md")).unwrap(), "a");
    let dir = root.join("backups/codex/items");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 20);
    let agent = root.join("reviewer.md");
    fs::write(&agent, "x").unwrap();
    let file_copy = store.backup_item(AgentId::Claude, &agent).unwrap().unwrap();
    assert_eq!(fs::read_to_string(file_copy).unwrap(), "x");
    // Pruning `tdd` must not count or remove `tdd.md` copies that share the prefix.
    let same_stem = root.join("tdd.md");
    fs::write(&same_stem, "agent").unwrap();
    for _ in 0..3 {
        store.backup_item(AgentId::Codex, &same_stem).unwrap();
    }
    store.backup_item(AgentId::Codex, &skill).unwrap();
    let names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.iter().filter(|n| n.starts_with("tdd.md.")).count(), 3);
    assert_eq!(names.len(), 23);
    let _ = fs::remove_dir_all(root);
}
