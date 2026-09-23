use super::*;
use crate::paths::scratch_dir;

/// A name no earlier run has used. A stub whose body carries it is this test's alone: this run
/// creates and warms its shared launcher, and a test that breaks that launcher breaks no other.
fn fresh_token(dir: &Path) -> String {
    dir.file_name().unwrap().to_string_lossy().into_owned()
}

/// Removes the shared launcher made for a body only this test uses.
fn discard_shared(stub: &CliStub) {
    let _ = fs::remove_file(shared_launcher_path(&stub.body()));
}

/// A hand-written launcher that notes, in a `warmed` file beside itself, a run with the warm-up
/// flag set, and then exits with `exit`.
fn warm_up_probe(dir: &Path, exit: i32) -> PathBuf {
    let path = dir.join(launcher_file_name("probe"));
    let body = if cfg!(windows) {
        format!("@echo off\r\nif defined {WARM_UP_VAR} echo warmed>\"%~dp0warmed\"\r\nexit /b {exit}\r\n")
    } else {
        format!("#!/bin/sh\nif [ -n \"${WARM_UP_VAR}\" ]; then printf warmed > \"$(dirname \"$0\")/warmed\"; fi\nexit {exit}\n")
    };
    fs::write(&path, body).unwrap();
    mark_executable(&path, PRIVATE_MODE).unwrap();
    path
}

#[test]
fn warm_up_starts_the_launcher_with_the_warm_up_flag_set() {
    let dir = scratch_dir("cli-stub-warm");
    let warmed = warm_up(&warm_up_probe(&dir, 0));
    assert_eq!(
        fs::read_to_string(dir.join("warmed"))
            .ok()
            .as_deref()
            .map(str::trim),
        Some("warmed"),
        "the warm-up must start the launcher, with the flag set"
    );
    assert!(warmed, "a launcher that exits 0 is warmed");
}

#[test]
fn warm_up_fails_for_a_launcher_that_fails() {
    let dir = scratch_dir("cli-stub-warm-fail");
    assert!(!warm_up(&warm_up_probe(&dir, 3)));
}

#[cfg(unix)]
#[test]
fn repeated_stubs_link_to_one_shared_launcher() {
    use std::os::unix::fs::MetadataExt;
    let (a, b) = (
        scratch_dir("cli-stub-share-a"),
        scratch_dir("cli-stub-share-b"),
    );
    let stub = CliStub::new("shared").stdout(&fresh_token(&a));
    let first = fs::metadata(stub.write(&a)).unwrap();
    let second = fs::metadata(stub.write(&b)).unwrap();
    let shared = fs::metadata(shared_launcher_path(&stub.body()));
    discard_shared(&stub);
    let shared = shared.expect("the first stub publishes the shared launcher");
    assert_eq!(
        (first.dev(), first.ino()),
        (shared.dev(), shared.ino()),
        "a stub must be the shared launcher, the file its warm-up already ran"
    );
    assert_eq!(
        (first.dev(), first.ino()),
        (second.dev(), second.ino()),
        "a second stub with the same body must be the same file"
    );
}

#[cfg(unix)]
#[test]
fn a_shared_launcher_cannot_be_rewritten_through_a_test_directory() {
    let dir = scratch_dir("cli-stub-sealed");
    let stub = CliStub::new("sealed").stdout(&fresh_token(&dir));
    let written = fs::write(stub.write(&dir), "#!/bin/sh\nexit 9\n");
    discard_shared(&stub);
    assert!(
        matches!(&written, Err(error) if error.kind() == io::ErrorKind::PermissionDenied),
        "writing through one test's link would change every other test's stub: {written:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_stub_that_cannot_replace_the_old_one_never_writes_through_its_link() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch_dir("cli-stub-stuck");
    let token = fresh_token(&dir);
    let old = CliStub::new("tool").stdout(&format!("{token}-old"));
    old.write(&dir);
    let shared = shared_launcher_path(&old.body());
    // Stands in for Windows, where a shared launcher stays writable and a link the OS still holds
    // cannot be removed.
    mark_executable(&shared, PRIVATE_MODE).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();

    let new = CliStub::new("tool").stdout(&format!("{token}-new"));
    let replaced = std::panic::catch_unwind(|| new.write(&dir));

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    let shared_now = fs::read_to_string(&shared).unwrap();
    discard_shared(&old);
    discard_shared(&new);
    assert_eq!(
        shared_now,
        old.body(),
        "every stub linked to the old launcher would run the new body"
    );
    assert!(
        replaced.is_err(),
        "a stub that cannot replace the old one must fail, not leave the old body in place"
    );
}

#[test]
fn each_launcher_keeps_its_state_in_its_own_directory() {
    let (first, second) = (scratch_dir("cli-stub-own-a"), scratch_dir("cli-stub-own-b"));
    let stub = CliStub::new("logger")
        .log_args("args.txt", false)
        .stdout(&fresh_token(&first));
    let runs = (
        stub.cli(&first).run(&["first"]),
        stub.cli(&second).run(&["second"]),
    );
    discard_shared(&stub);
    runs.0.unwrap();
    runs.1.unwrap();
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
    let token = fresh_token(&dir);
    let (copied, logged) = (format!("{token}.copied"), format!("{token}.args"));
    fs::write(dir.join("source"), "fixture").unwrap();
    let stub = CliStub::new("inert")
        .copy("source", &copied)
        .log_args(&logged, false);
    stub.write(&dir);
    // The warm-up runs the shared file, so a body that ignored the flag would act beside it.
    let shared_dir = shared_launcher_path(&stub.body())
        .parent()
        .unwrap()
        .to_path_buf();
    let acted: Vec<PathBuf> = [&dir, &shared_dir]
        .into_iter()
        .flat_map(|place| [place.join(&copied), place.join(&logged)])
        .filter(|trace| trace.exists())
        .collect();
    discard_shared(&stub);
    let _ = fs::remove_file(shared_dir.join(&logged));
    assert!(acted.is_empty(), "the stub ran and left {acted:?}");
}

#[test]
fn a_stub_that_fails_when_run_still_warms_up() {
    let token = fresh_token(&scratch_dir("cli-stub-failing"));
    // Run for real, this body fails; its warm-up run must exit before the body and succeed.
    let stub = CliStub::new("failing").stdout(&token).exit(3);
    let shared = shared_launcher(&stub.body());
    discard_shared(&stub);
    assert!(
        shared.is_some(),
        "a stub that fails when run must still be shareable"
    );
}

#[test]
fn stubs_with_different_bodies_stay_apart() {
    let dir = scratch_dir("cli-stub-apart");
    let token = fresh_token(&dir);
    let alpha = CliStub::new("alpha").stdout(&format!("{token}-alpha"));
    let beta = CliStub::new("beta").stdout(&format!("{token}-beta"));
    let outputs = (alpha.cli(&dir).run(&[]), beta.cli(&dir).run(&[]));
    discard_shared(&alpha);
    discard_shared(&beta);
    assert_eq!(outputs.0.unwrap().trim(), format!("{token}-alpha"));
    assert_eq!(outputs.1.unwrap().trim(), format!("{token}-beta"));
}

#[test]
fn a_stub_written_twice_under_one_name_runs_the_second_body() {
    let dir = scratch_dir("cli-stub-rewrite");
    let token = fresh_token(&dir);
    let before = CliStub::new("tool").stdout(&format!("{token}-before"));
    let after = CliStub::new("tool").stdout(&format!("{token}-after"));
    before.write(&dir);
    let out = after.cli(&dir).run(&[]);
    discard_shared(&before);
    discard_shared(&after);
    assert_eq!(out.unwrap().trim(), format!("{token}-after"));
}

#[test]
fn a_foreign_file_under_a_launchers_name_is_never_run() {
    let dir = scratch_dir("cli-stub-foreign");
    let token = fresh_token(&dir);
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
