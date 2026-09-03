use super::*;
use crate::cli::AgentCli;
use crate::cli_stub::CliStub;
use crate::paths::scratch_dir;
use std::cell::Cell;
use std::fs;
use std::time::{Duration, Instant};

#[test]
fn a_logged_in_gh_hands_over_its_trimmed_token() {
    let dir = scratch_dir("gh-token-ok");
    let cli = CliStub::new("gh")
        .log_args("args.txt", false)
        .stdout("gho_abc123")
        .cli(&dir);
    let token = read_token_with(&cli, TOKEN_DEADLINE).unwrap();
    assert_eq!(token.as_str(), "gho_abc123");
    let args = fs::read_to_string(dir.join("args.txt")).unwrap();
    assert!(args.contains("auth token"), "{args}");
    assert!(args.contains("--hostname github.com"), "{args}");
}

#[test]
fn a_signed_out_gh_is_not_logged_in_and_its_stderr_stays_private() {
    let dir = scratch_dir("gh-token-signed-out");
    let cli = CliStub::new("gh")
        .stderr("no oauth token found for github.com: secret-path")
        .exit(1)
        .cli(&dir);
    let error = read_token_with(&cli, TOKEN_DEADLINE).unwrap_err();
    assert_eq!(error, TokenError::NotLoggedIn);
    assert!(!format!("{error:?}").contains("secret-path"));
}

#[test]
fn an_empty_or_multi_word_answer_is_not_a_token() {
    let dir = scratch_dir("gh-token-empty");
    let cli = CliStub::new("gh").stdout("").cli(&dir);
    assert_eq!(
        read_token_with(&cli, TOKEN_DEADLINE).unwrap_err(),
        TokenError::NotLoggedIn
    );
    let cli = CliStub::new("gh").stdout("not a token").cli(&dir.join("b"));
    assert_eq!(
        read_token_with(&cli, TOKEN_DEADLINE).unwrap_err(),
        TokenError::NotLoggedIn
    );
}

#[test]
fn a_missing_gh_is_reported_as_missing() {
    let dir = scratch_dir("gh-token-missing");
    let cli = AgentCli::new(dir.join("gh").to_string_lossy().as_ref());
    assert_eq!(
        read_token_with(&cli, TOKEN_DEADLINE).unwrap_err(),
        TokenError::GhMissing
    );
}

#[test]
fn a_hung_gh_is_killed_at_the_deadline() {
    let dir = scratch_dir("gh-token-hang");
    let cli = CliStub::new("gh").sleep(5).cli(&dir);
    let started = Instant::now();
    assert_eq!(
        read_token_with(&cli, Duration::from_millis(100)).unwrap_err(),
        TokenError::TimedOut
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn the_memo_reads_once_until_cleared() {
    let memo = GhTokenMemo::new();
    let reads = Cell::new(0_u32);
    let read = || {
        reads.set(reads.get() + 1);
        Ok(GhToken("gho_memo".into()))
    };
    assert_eq!(memo.lookup(read).unwrap().as_str(), "gho_memo");
    assert_eq!(memo.lookup(read).unwrap().as_str(), "gho_memo");
    assert_eq!(reads.get(), 1);
    memo.clear();
    assert_eq!(memo.lookup(read).unwrap().as_str(), "gho_memo");
    assert_eq!(reads.get(), 2);
}

#[test]
fn the_memo_never_keeps_a_failure() {
    let memo = GhTokenMemo::new();
    let reads = Cell::new(0_u32);
    let read = || {
        reads.set(reads.get() + 1);
        Err(TokenError::NotLoggedIn)
    };
    assert_eq!(memo.lookup(read).unwrap_err(), TokenError::NotLoggedIn);
    assert_eq!(memo.lookup(read).unwrap_err(), TokenError::NotLoggedIn);
    assert_eq!(reads.get(), 2);
}

#[test]
fn debug_output_never_contains_the_token() {
    let token = GhToken("gho_very_secret".into());
    let printed = format!("{token:?} {:?}", Ok::<_, TokenError>(token.clone()));
    assert!(!printed.contains("very_secret"), "{printed}");
    assert!(printed.contains("redacted"), "{printed}");
}
