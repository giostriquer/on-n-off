use super::*;
use crate::paths::scratch_dir;

#[cfg(unix)]
#[test]
fn repeated_stubs_share_the_launcher_file_the_first_one_warmed() {
    use std::os::unix::fs::MetadataExt;
    let stub = CliStub::new("shared").stdout("same-body");
    let first = fs::metadata(stub.write(&scratch_dir("cli-stub-share-a"))).unwrap();
    let second = fs::metadata(stub.write(&scratch_dir("cli-stub-share-b"))).unwrap();
    assert_eq!(
        (first.dev(), first.ino()),
        (second.dev(), second.ino()),
        "a second stub with the same body must be the file the first one already ran"
    );
}

#[cfg(unix)]
#[test]
fn a_shared_launcher_cannot_be_rewritten_through_a_test_directory() {
    let path = CliStub::new("sealed").write(&scratch_dir("cli-stub-sealed"));
    let error = fs::write(&path, "#!/bin/sh\nexit 9\n").unwrap_err();
    assert_eq!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied,
        "writing through one test's link would change every other test's stub"
    );
}

#[test]
fn each_launcher_keeps_its_state_in_its_own_directory() {
    let stub = CliStub::new("logger").log_args("args.txt", false);
    let (first, second) = (scratch_dir("cli-stub-own-a"), scratch_dir("cli-stub-own-b"));
    stub.cli(&first).run(&["first"]).unwrap();
    stub.cli(&second).run(&["second"]).unwrap();
    assert_eq!(
        fs::read_to_string(first.join("args.txt")).unwrap().trim(),
        "first"
    );
    assert_eq!(
        fs::read_to_string(second.join("args.txt")).unwrap().trim(),
        "second"
    );
}

#[test]
fn writing_a_stub_does_not_run_it() {
    let dir = scratch_dir("cli-stub-inert");
    fs::write(dir.join("source"), "fixture").unwrap();
    CliStub::new("inert")
        .copy("source", "copied")
        .log_args("args.txt", false)
        .write(&dir);
    assert!(!dir.join("copied").exists());
    assert!(!dir.join("args.txt").exists());
}

#[test]
fn stubs_with_different_bodies_stay_apart() {
    let dir = scratch_dir("cli-stub-apart");
    let alpha = CliStub::new("alpha").stdout("alpha-out").cli(&dir);
    let beta = CliStub::new("beta").stdout("beta-out").cli(&dir);
    assert_eq!(alpha.run(&[]).unwrap().trim(), "alpha-out");
    assert_eq!(beta.run(&[]).unwrap().trim(), "beta-out");
}

#[test]
fn a_stub_written_twice_under_one_name_runs_the_second_body() {
    let dir = scratch_dir("cli-stub-rewrite");
    CliStub::new("tool").stdout("before").write(&dir);
    let cli = CliStub::new("tool").stdout("after").cli(&dir);
    assert_eq!(cli.run(&[]).unwrap().trim(), "after");
}

#[test]
fn a_launcher_is_warmed_by_a_run_that_skips_its_behaviour() {
    // Run for real, this body logs its argv and fails; its warm-up run must do neither.
    let stub = CliStub::new("failing").log_args("args.txt", false).exit(3);
    assert!(
        shared_launcher(&stub.body()).is_some(),
        "the warm-up run must exit before the body and succeed"
    );
}

#[test]
fn a_foreign_file_under_a_launchers_name_is_never_run() {
    let dir = scratch_dir("cli-stub-foreign");
    let token = dir.file_name().unwrap().to_string_lossy().into_owned();
    let stub = CliStub::new("foreign").stdout(&token);
    let shared = shared_launcher_path(&stub.body());
    fs::create_dir_all(shared.parent().unwrap()).unwrap();
    // A launcher that runs, just not this one.
    let impostor = CliStub::new("foreign").stdout("impostor").body();
    fs::write(&shared, impostor).unwrap();
    mark_executable(&shared, PRIVATE_MODE).unwrap();

    let out = stub.cli(&dir).run(&[]);

    let _ = fs::remove_file(&shared);
    assert_eq!(out.unwrap().trim(), token);
}
