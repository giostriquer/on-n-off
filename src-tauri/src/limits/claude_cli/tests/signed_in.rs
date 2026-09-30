//! The signed-in account's card as Claude Code reports it for the user's own config dir
//! (`read_signed_in`): the account `.claude.json` names, before and after the read, and what the
//! card says when there is no report.
use super::*;

/// The signed-in card from a stand-in `claude` built in `dir`.
fn signed_in(dir: &tempfile::TempDir, cli: &AgentCli) -> ProviderLimitsDto {
    read_signed_in_within(&|| cli.command(), &dir.path().join(".claude.json"), ANSWER)
}

#[test]
fn a_report_reads_as_the_signed_in_card_of_the_account_the_config_dir_names() {
    let (dir, cli) = home(&config("user", "team"), REPORT, CliStub::new("claude"));

    let card = signed_in(&dir, &cli);

    assert_eq!(card.status, LimitsStatus::Ok);
    assert!(card.current_account);
    // Known by its scoped key, as its saved profile's card is, so the two are one card.
    let account = card.account.expect("the signed-in account");
    assert_eq!(account.id, identity().observation_key());
    assert_eq!(account.legacy_id.as_deref(), Some("user"));
    assert_eq!(account.label.as_deref(), Some("you@example.com"));
    assert_eq!(card.reading.plan.as_deref(), Some("max ×20"));
    assert_eq!(card.reading.windows.len(), 3);
}

#[test]
fn an_account_without_an_organization_is_known_by_its_own_id() {
    let config = serde_json::json!({
        "oauthAccount": {"accountUuid": "user", "emailAddress": "you@example.com"}
    })
    .to_string();
    let (dir, cli) = home(&config, REPORT, CliStub::new("claude"));

    let card = signed_in(&dir, &cli);

    assert_eq!(card.status, LimitsStatus::Ok);
    assert_eq!(card.account.expect("the account").id, "user");
}

#[test]
fn a_signed_out_config_dir_asks_for_a_sign_in() {
    let (dir, usage) = home("{}", "", CliStub::new("claude"));
    let status_dir = tempfile::tempdir().unwrap();
    let status = CliStub::new("claude")
        .stdout(r#"{"loggedIn":false,"authMethod":"none"}"#)
        .exit(1)
        .cli(status_dir.path());

    let card = read_signed_in_within(
        &usage_then_status(&usage, &status),
        &dir.path().join(".claude.json"),
        ANSWER,
    );

    assert_eq!(card.status, LimitsStatus::SignedOut);
    assert_eq!(
        card.message.as_deref(),
        Some("Sign in with `claude` to see subscription limits.")
    );
    assert_eq!(card.account.expect("the account").id, "default");
}

#[test]
fn a_failed_read_keeps_the_account_so_its_remembered_card_stands_in() {
    let (dir, cli) = home(
        &config("user", "team"),
        REPORT,
        CliStub::new("claude").exit(1),
    );

    let card = signed_in(&dir, &cli);

    assert_eq!(card.status, LimitsStatus::Failed);
    assert_eq!(
        card.message.as_deref(),
        Some("Claude Code could not report usage.")
    );
    assert_eq!(
        card.account.expect("the account").id,
        identity().observation_key()
    );
}

#[test]
fn no_report_from_a_config_dir_still_signed_in_is_a_failed_read() {
    let (dir, usage) = home(&config("user", "team"), "", CliStub::new("claude"));
    let status_dir = tempfile::tempdir().unwrap();
    let status = CliStub::new("claude")
        .stdout(r#"{"loggedIn":true,"authMethod":"claude.ai"}"#)
        .cli(status_dir.path());

    let card = read_signed_in_within(
        &usage_then_status(&usage, &status),
        &dir.path().join(".claude.json"),
        ANSWER,
    );

    assert_eq!(card.status, LimitsStatus::Failed);
    assert_eq!(
        card.message.as_deref(),
        Some("Claude Code reported no usage.")
    );
}

/// A report with no window Claude Code's reader knows is no reading: the card fails, and shows
/// what it remembers, rather than an account with no windows.
#[test]
fn a_report_with_no_readable_window_is_a_failed_read() {
    let output = r#"{"type":"assistant","usage_report":{"rate_limits":{"limits":[{"kind":"lunar","group":"lunar","percent":1}]}}}"#;
    let (dir, cli) = home(&config("user", "team"), output, CliStub::new("claude"));

    let card = signed_in(&dir, &cli);

    assert_eq!(card.status, LimitsStatus::Failed);
    assert_eq!(
        card.message.as_deref(),
        Some("Claude Code reported no usage.")
    );
}

/// The account `.claude.json` names must be the same one after the read, user and organization
/// alike: one that changed, appeared or went while Claude Code ran is not shown.
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
        let cli = CliStub::new("claude")
            .copy("after.json", ".claude.json")
            .stdout_file("report.jsonl")
            .cli(dir.path());

        let card = signed_in(&dir, &cli);

        assert_eq!(card.status, LimitsStatus::Failed, "{after}");
        assert_eq!(
            card.message.as_deref(),
            Some("The signed-in Claude account changed while its usage was read."),
            "{after}"
        );
        assert!(
            card.reading.windows.is_empty(),
            "showed the report: {after}"
        );
    }
}

/// A config dir that names no account, before and after, is read as the one default account.
#[test]
fn a_config_dir_naming_no_account_reads_as_the_default_account() {
    let (dir, cli) = home("{}", REPORT, CliStub::new("claude"));

    let card = signed_in(&dir, &cli);

    assert_eq!(card.status, LimitsStatus::Ok);
    assert_eq!(card.account.expect("the account").id, "default");
    assert_eq!(card.reading.windows.len(), 3);
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

    let card = signed_in(&dir, &cli);

    assert_eq!(card.status, LimitsStatus::Failed);
    assert_eq!(card.message.as_deref(), Some(OUTDATED));
    // Asked once, with the flag, and never again without it.
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

    let card = signed_in(&dir, &cli);

    assert_eq!(
        card.message.as_deref(),
        Some("Claude Code could not report usage.")
    );
}
