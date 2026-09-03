impl AgentCli {
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self { timeout, ..self }
    }
}

use super::*;
use crate::cli_stub::CliStub;
use std::path::PathBuf;
use std::time::Instant;

fn stub_dir() -> std::path::PathBuf {
    crate::paths::scratch_dir("on-n-off-cli")
}

#[test]
fn success_returns_stdout() {
    let out = CliStub::new("ok")
        .stdout("{\"ok\":true}")
        .cli(&stub_dir())
        .run(&["plugin", "list"])
        .expect("ok");
    assert!(out.contains("ok"));
}

#[test]
fn non_zero_returns_stderr() {
    let err = CliStub::new("fail")
        .stderr("boom")
        .exit(2)
        .cli(&stub_dir())
        .run(&["plugin", "enable", "x"])
        .expect_err("fail");
    assert_eq!(err.kind, ErrorKind::Message);
    assert!(err.message.contains("boom"));
}

#[test]
fn timeout_kills_the_process() {
    let cli = CliStub::new("slow").sleep(7).cli(&stub_dir());
    let started = Instant::now();
    let err = cli
        .with_timeout(Duration::from_millis(200))
        .run(&["plugin", "enable", "x"])
        .expect_err("timeout");
    assert!(err.message.to_lowercase().contains("timed out"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "timeout returned after {:?}",
        started.elapsed()
    );
}

#[test]
fn drains_chatty_stdout_and_stderr_while_the_process_runs() {
    let out = CliStub::new("chatty")
        .chatty(5000)
        .stdout("complete")
        .cli(&stub_dir())
        .with_timeout(Duration::from_secs(5))
        .run(&[])
        .expect("chatty child must not block on full output pipes");
    assert!(out.contains("complete"));
}

#[cfg(unix)]
#[test]
fn non_executable_launcher_gets_a_permission_hint() {
    // Not an agent name: agent-named binaries fall through to the user's settings overrides.
    let path = stub_dir().join("noexec-tool");
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    let err = AgentCli::new(path.to_string_lossy().as_ref())
        .run(&["--version"])
        .expect_err("permission denied");
    assert!(
        err.message.contains("not executable") && err.message.contains("noexec-tool"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn children_get_the_cli_search_path() {
    let printed = CliStub::new("path-echo")
        .print_env("PATH")
        .cli(&stub_dir())
        .run(&[])
        .expect("path-echo");
    let printed_dirs: Vec<PathBuf> = std::env::split_paths(printed.trim()).collect();
    let search_path = crate::cli_locate::cli_search_path();
    for dir in search_path {
        assert!(
            printed_dirs.contains(dir),
            "child PATH lacks {dir:?}: {printed}"
        );
    }
    // The well-known tier (not just the process PATH) must reach the child. Compared by
    // suffix because other tests re-point ON_N_OFF_HOME while this process runs.
    assert!(
        search_path
            .iter()
            .any(|dir| dir.ends_with(std::path::Path::new(".local").join("bin"))),
        "well-known dirs missing from the search path: {search_path:?}"
    );
}

#[test]
fn missing_binary_is_cli_missing() {
    let err = AgentCli::new("on-n-off-no-such-cli.exe")
        .run(&["plugin", "list"])
        .expect_err("missing");
    assert_eq!(err.kind, ErrorKind::CliMissing);
}
