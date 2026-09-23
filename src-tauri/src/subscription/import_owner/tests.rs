use super::*;
use std::{process::Stdio, sync::Arc, time::Duration};

#[test]
fn shutdown_reaps_import_before_removing_its_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let scratch = private_scratch(root.path()).unwrap();
    let path = scratch.path().to_path_buf();
    std::fs::write(path.join("source"), "disposable cookie fixture").unwrap();
    let helper = crate::cli_stub::CliStub::new("import")
        .copy("source", "snapshot")
        .sleep(10)
        .write(&path);
    let owner = Arc::new(ImportOwner::default());
    let worker_owner = Arc::clone(&owner);
    let worker = std::thread::spawn(move || {
        let mut command = std::process::Command::new(helper);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        worker_owner.run(command, scratch)
    });
    // How soon the helper starts is not what this test is about, and on a loaded machine a
    // launcher can take seconds to start. Wait for the snapshot itself, and stop early only if
    // the helper has already gone; the cap only keeps a regression from hanging the run.
    let waiting = std::time::Instant::now();
    while !path.join("snapshot").exists() && !worker.is_finished() {
        assert!(
            waiting.elapsed() < Duration::from_secs(60),
            "the import helper never copied its snapshot"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        path.join("snapshot").exists(),
        "the import helper finished without copying its snapshot"
    );
    owner.shutdown();
    assert!(
        !path.exists(),
        "shutdown must remove the credential snapshot before returning"
    );
    assert!(matches!(
        worker.join().unwrap().unwrap(),
        crate::process::CommandOutcome::TimedOut
    ));
    let scratch = private_scratch(root.path()).unwrap();
    assert!(owner
        .run(std::process::Command::new("unused"), scratch)
        .is_err());
}

#[test]
fn recovery_preserves_live_leases_and_removes_only_abandoned_imports() {
    let root = tempfile::tempdir().unwrap();
    let live = private_scratch(root.path()).unwrap();
    let scratch = private_scratch(root.path()).unwrap();
    let abandoned = scratch.directory.keep();
    drop(scratch._lease);
    let unrelated = root.path().join("other");
    std::fs::create_dir(&unrelated).unwrap();
    recover(root.path());
    assert!(live.path().exists());
    assert!(!abandoned.exists());
    assert!(unrelated.exists());
}
