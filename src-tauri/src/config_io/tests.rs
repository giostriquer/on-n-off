use super::*;
use crate::dto::ErrorKind;
use serde_json::json;

impl ConfigIo {
    fn force_invalid_commit(&self, agent: AgentId, path: &Path) -> Result<(), AdapterError> {
        let original = read_or_empty(path)?;
        let backups = self.back_up(agent, &[path.to_path_buf()])?;
        self.commit(path, backups.of(path)?, &original, "{not json", |text| {
            serde_json::from_str::<JsonValue>(text).map(|_| ())
        })
    }
}

#[test]
fn unreadable_write_restores_backup() {
    let root = crate::paths::scratch_dir("on-n-off-restore");
    let path = root.join("settings.json");
    fs::write(&path, "{\"keep\":true}").unwrap();
    let io = ConfigIo::at(root.join("backups"));
    let err = io
        .force_invalid_commit(AgentId::Claude, &path)
        .expect_err("invalid commit");
    assert_eq!(err.kind, ErrorKind::Write);
    let value: JsonValue = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value, json!({"keep": true}));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_backup_that_cannot_be_written_refuses_the_change_and_a_missing_file_needs_none() {
    let root = crate::paths::scratch_dir("on-n-off-back-up");
    let present = root.join("settings.json");
    let missing = root.join("installed.json");
    fs::write(&present, "{}").unwrap();

    let io = ConfigIo::at(root.join("backups"));
    let backups = io
        .back_up(AgentId::Muse, &[present.clone(), missing.clone()])
        .unwrap();
    assert!(backups.of(&present).unwrap().is_some());
    assert_eq!(backups.of(&missing).unwrap(), None);
    assert_eq!(
        fs::read(backups.of(&present).unwrap().unwrap()).unwrap(),
        b"{}"
    );

    fs::write(root.join("blocked"), "file").unwrap();
    let error = ConfigIo::at(root.join("blocked"))
        .back_up(AgentId::Muse, &[present])
        .err()
        .expect("refused");
    assert_eq!(error.kind, ErrorKind::Write);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_patch_whose_file_the_change_did_not_back_up_is_refused_and_writes_nothing() {
    let root = crate::paths::scratch_dir("on-n-off-uncovered-patch");
    let path = root.join("settings.json");
    fs::write(&path, r#"{"keep":true}"#).unwrap();
    let io = ConfigIo::at(root.join("backups"));
    let other = io.back_up(AgentId::Claude, &[]).unwrap();

    let answer = io.patch(
        &path,
        &other,
        Edit::json(|root| {
            root["keep"] = json!(false);
            Ok(())
        }),
    );

    let error = answer.expect_err("a patch outside the change's backups");
    assert_eq!(error.kind, ErrorKind::Write);
    assert_eq!(
        error.path.as_deref(),
        Some(path.display().to_string().as_str())
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"keep":true}"#);
    assert!(!root.join("backups").exists());
    let _ = fs::remove_dir_all(root);
}
