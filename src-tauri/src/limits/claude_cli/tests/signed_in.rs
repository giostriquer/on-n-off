use super::*;

fn signed_in(dir: &tempfile::TempDir, cli: &AgentCli) -> Read {
    read_signed_in_within(
        &|_| cli.command(),
        &dir.path().join(".claude.json"),
        &dir.path().join("usage"),
        ANSWER,
    )
}

#[test]
fn a_report_reads_as_the_signed_in_card_of_the_account_the_config_dir_names() {
    let (dir, cli) = home(&config("user", "team"), REPORT, CliStub::new("claude"));

    let read = signed_in(&dir, &cli);

    assert_eq!(
        read.account.map(|account| account.id),
        Some(identity().observation_key())
    );
    let answer = read.answer.expect("the signed-in account's reading");
    let account = answer.account.expect("the signed-in account");
    assert_eq!(account.id, identity().observation_key());
    assert_eq!(account.legacy_id.as_deref(), Some("user"));
    assert_eq!(account.label.as_deref(), Some("you@example.com"));
    assert_eq!(answer.reading.plan.as_deref(), Some("max ×20"));
    assert_eq!(answer.reading.windows.len(), 3);
}

#[test]
fn an_account_without_an_organization_is_known_by_its_own_id() {
    let config = serde_json::json!({
        "oauthAccount": {"accountUuid": "user", "emailAddress": "you@example.com"}
    })
    .to_string();
    let (dir, cli) = home(&config, REPORT, CliStub::new("claude"));

    let answer = signed_in(&dir, &cli).answer.expect("a reading");

    assert_eq!(answer.account.expect("the account").id, "user");
}

#[test]
fn a_signed_out_config_dir_asks_for_a_sign_in() {
    let (dir, usage) = home("{}", "", CliStub::new("claude"));
    let status_dir = tempfile::tempdir().unwrap();
    let status = CliStub::new("claude")
        .stdout(r#"{"loggedIn":false,"authMethod":"none"}"#)
        .exit(1)
        .cli(status_dir.path());

    let next = usage_then_status(&usage, &status);
    let read = read_signed_in_within(
        &|_| next(),
        &dir.path().join(".claude.json"),
        &dir.path().join("usage"),
        ANSWER,
    );

    assert_eq!(read.answer.unwrap_err(), ReadFailure::SignedOut);
}

#[test]
fn a_failed_read_keeps_the_account_so_its_remembered_card_stands_in() {
    let (dir, cli) = home(
        &config("user", "team"),
        REPORT,
        CliStub::new("claude").exit(1),
    );

    let read = signed_in(&dir, &cli);

    assert_eq!(
        read.answer.unwrap_err(),
        ReadFailure::Unavailable(UNREPORTED.into())
    );
    assert_eq!(
        read.account.expect("the account").id,
        identity().observation_key()
    );
}

#[test]
fn no_report_from_a_config_dir_still_signed_in_answers_no_window() {
    let (dir, usage) = home(&config("user", "team"), "", CliStub::new("claude"));
    let status_dir = tempfile::tempdir().unwrap();
    let status = CliStub::new("claude")
        .stdout(r#"{"loggedIn":true,"authMethod":"claude.ai"}"#)
        .cli(status_dir.path());

    let next = usage_then_status(&usage, &status);
    let read = read_signed_in_within(
        &|_| next(),
        &dir.path().join(".claude.json"),
        &dir.path().join("usage"),
        ANSWER,
    );

    assert!(read.answer.expect("an answer").reading.windows.is_empty());
}

#[test]
fn a_report_with_no_readable_window_answers_no_window() {
    let output = r#"{"type":"assistant","usage_report":{"rate_limits":{"limits":[{"kind":"lunar","group":"lunar","percent":1}]}}}"#;
    let (dir, cli) = home(&config("user", "team"), output, CliStub::new("claude"));

    let read = signed_in(&dir, &cli);

    assert!(read.answer.expect("an answer").reading.windows.is_empty());
}

#[test]
fn a_report_after_which_the_config_dir_names_another_account_is_not_shown() {
    let empty = "{}".to_string();
    for after in [
        config("someone-else", "team"),
        config("user", "other-team"),
        empty,
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".claude.json"), config("user", "team")).unwrap();
        std::fs::write(dir.path().join("after.json"), &after).unwrap();
        std::fs::write(dir.path().join("report.jsonl"), REPORT).unwrap();
        std::fs::create_dir_all(dir.path().join("usage")).unwrap();
        std::fs::write(
            dir.path().join("usage").join(".claude.json"),
            config("user", "team"),
        )
        .unwrap();
        let cli = CliStub::new("claude")
            .copy("after.json", ".claude.json")
            .stdout_file("report.jsonl")
            .cli(dir.path());

        let read = signed_in(&dir, &cli);

        assert_eq!(
            read.answer.unwrap_err(),
            ReadFailure::AccountChanged,
            "{after}"
        );
        assert!(
            !dir.path().join("usage").exists(),
            "kept the usage dir that read while the account changed: {after}"
        );
    }
}

#[test]
fn a_config_dir_naming_no_account_reads_as_the_default_account() {
    let (dir, cli) = home("{}", REPORT, CliStub::new("claude"));

    let read = signed_in(&dir, &cli);

    assert_eq!(
        read.account.as_ref().map(|a| a.id.as_str()),
        Some("default")
    );
    let answer = read.answer.expect("a reading");
    assert_eq!(answer.account.expect("the account").id, "default");
    assert_eq!(answer.reading.windows.len(), 3);
}

#[test]
fn a_claude_code_too_old_to_leave_customizations_out_asks_for_an_update() {
    let (dir, cli) = home(
        &config("user", "team"),
        REPORT,
        CliStub::new("claude")
            .log_args("args.txt", true)
            .stderr("error: unknown option --safe-mode")
            .exit(1),
    );

    let read = signed_in(&dir, &cli);

    assert_eq!(
        read.answer.unwrap_err(),
        ReadFailure::Unavailable(OUTDATED.into())
    );
    let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
    let runs: Vec<&str> = args.lines().collect();
    assert_eq!(runs.len(), 1, "{args}");
    assert!(runs[0].contains("--safe-mode"), "{args}");
}

#[test]
fn another_unknown_option_is_not_read_as_an_outdated_claude_code() {
    let (dir, cli) = home(
        &config("user", "team"),
        REPORT,
        CliStub::new("claude")
            .stderr("error: unknown option --verbose")
            .exit(1),
    );

    let read = signed_in(&dir, &cli);

    assert_eq!(
        read.answer.unwrap_err(),
        ReadFailure::Unavailable(UNREPORTED.into())
    );
}

fn usage_dir_left_with(native: &str, usage_config: &str) -> (tempfile::TempDir, AgentCli) {
    let (dir, cli) = home(
        native,
        REPORT,
        CliStub::new("claude").copy("usage/.claude.json", "seen.json"),
    );
    let backups = dir.path().join("usage").join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    std::fs::write(dir.path().join("usage").join(".claude.json"), usage_config).unwrap();
    std::fs::write(backups.join(".claude.json.backup"), usage_config).unwrap();
    (dir, cli)
}

#[test]
fn claude_code_never_reports_from_a_usage_dir_that_names_another_account() {
    let signed_in_user = config("user", "team");
    let no_account = "{}".to_string();
    for (native, left) in [
        (&signed_in_user, config("someone-else", "team")),
        (&signed_in_user, config("user", "other-team")),
        (&signed_in_user, no_account.clone()),
        (&no_account, config("user", "team")),
    ] {
        let (dir, cli) = usage_dir_left_with(native, &left);

        let read = signed_in(&dir, &cli);

        assert!(read.answer.is_ok(), "{native} / {left}");
        assert!(
            !dir.path().join("seen.json").exists(),
            "Claude Code reported from a usage dir naming another account: {native} / {left}"
        );
        assert!(
            !dir.path().join("usage").join("backups").exists(),
            "kept a backup of another account's usage dir: {native} / {left}"
        );
    }
}

#[test]
fn a_usage_dir_that_names_the_signed_in_account_is_kept() {
    for kept in [config("user", "team"), "{}".to_string()] {
        let (dir, cli) = usage_dir_left_with(&kept, &kept);

        let read = signed_in(&dir, &cli);

        assert!(read.answer.is_ok(), "{kept}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seen.json")).unwrap(),
            kept
        );
        assert!(dir.path().join("usage").join("backups").exists(), "{kept}");
    }
}

#[test]
fn a_usage_dir_that_cannot_be_cleared_fails_the_read_without_asking_claude_code() {
    let (dir, cli) = home(
        &config("user", "team"),
        REPORT,
        CliStub::new("claude").log_args("args.txt", false),
    );
    std::fs::write(dir.path().join("usage"), "a file where the dir belongs").unwrap();

    let read = signed_in(&dir, &cli);

    assert!(
        matches!(&read.answer, Err(ReadFailure::Unavailable(why))
            if why.starts_with("Could not clear Claude Code's usage config dir")),
        "{:?}",
        read.answer
    );
    assert_eq!(
        read.account.expect("the account").id,
        identity().observation_key()
    );
    assert!(!dir.path().join("args.txt").exists());
}

#[test]
fn every_failure_after_the_config_dir_names_the_account_keeps_it() {
    use crate::limits::outcome::tests::{
        every_failure_keeps_the_account_its_answer_names, Expected,
    };
    let account = config("user", "team");
    let failing_with = |stub: CliStub, stdout: &str| {
        let (dir, cli) = home(&account, stdout, stub);
        signed_in(&dir, &cli)
    };
    let changed = {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".claude.json"), &account).unwrap();
        std::fs::write(
            dir.path().join("after.json"),
            config("someone-else", "team"),
        )
        .unwrap();
        std::fs::write(dir.path().join("report.jsonl"), REPORT).unwrap();
        let cli = CliStub::new("claude")
            .copy("after.json", ".claude.json")
            .stdout_file("report.jsonl")
            .cli(dir.path());
        signed_in(&dir, &cli)
    };
    let uncleared = {
        let (dir, cli) = home(&account, REPORT, CliStub::new("claude"));
        std::fs::write(dir.path().join("usage"), "a file where the dir belongs").unwrap();
        signed_in(&dir, &cli)
    };
    let (dir, cli) = home(&account, REPORT, CliStub::new("claude"));
    let answered = signed_in(&dir, &cli);

    every_failure_keeps_the_account_its_answer_names(
        AgentId::Claude,
        answered,
        vec![
            (
                failing_with(CliStub::new("claude").exit(1), REPORT),
                (|failure| *failure == ReadFailure::Unavailable(UNREPORTED.into())) as Expected,
            ),
            (
                failing_with(
                    CliStub::new("claude")
                        .stderr("error: unknown option --safe-mode")
                        .exit(1),
                    REPORT,
                ),
                |failure| *failure == ReadFailure::Unavailable(OUTDATED.into()),
            ),
            (
                failing_with(
                    CliStub::new("claude"),
                    r#"{"type":"assistant","usage_report":{"rate_limits":{"limits":[]}}}"#,
                ),
                |failure| *failure == ReadFailure::NoUsage,
            ),
            (changed, |failure| *failure == ReadFailure::AccountChanged),
            (uncleared, |failure| {
                matches!(failure, ReadFailure::Unavailable(why)
                    if why.starts_with("Could not clear Claude Code's usage config dir"))
            }),
        ],
    );
}
