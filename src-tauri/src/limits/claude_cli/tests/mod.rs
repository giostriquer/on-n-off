//! A Claude account's usage as Claude Code reports it (`read_usage`): the windows of its
//! `usage_report`, whose account the config dir names, read by a stand-in `claude`.
use super::*;
use crate::cli::AgentCli;
use crate::cli_stub::CliStub;
use crate::dto::{AgentId, LimitWindowKind, LimitsStatus};
use std::time::Duration;

/// What `claude -p /usage --output-format stream-json --verbose` prints, trimmed to the lines the
/// reader looks at: the session's start, the local command's answer and the result, with a stray
/// line the reader has to step over.
const REPORT: &str = r#"{"type":"system","subtype":"init","apiKeySource":"none","claude_code_version":"2.1.284"}
a line that is not an event
{"type":"assistant","local_command_run":"usage","message":{"content":"Current session: 12% used"},"usage_report":{"rate_limits":{"limits":[{"kind":"session","group":"session","percent":12,"resets_at":"2026-09-29T18:00:00.006917+00:00","scope":null,"severity":"normal","is_active":true},{"kind":"weekly_all","group":"weekly","percent":34,"resets_at":"2026-10-05T09:00:00.006938+00:00","scope":null,"severity":"normal","is_active":false},{"kind":"weekly_scoped","group":"weekly","percent":5,"resets_at":"2026-10-05T09:00:00+00:00","scope":{"model":{"display_name":"Fable"},"surface":null},"severity":"normal","is_active":false}],"extra_usage":{"is_enabled":false}},"session":{"total_cost_usd":0}}}
{"type":"result","subtype":"success","is_error":false,"num_turns":0,"duration_api_ms":0,"total_cost_usd":0}"#;

fn identity() -> Identity {
    Identity {
        provider: AgentId::Claude,
        user_id: "user".into(),
        workspace_id: "team".into(),
    }
}

/// The `.claude.json` Claude Code keeps in a config dir, naming `user` in `org`.
fn config(user: &str, org: &str) -> String {
    serde_json::json!({
        "oauthAccount": {
            "accountUuid": user,
            "organizationUuid": org,
            "emailAddress": "you@example.com",
            "organizationType": "claude_max",
            "organizationRateLimitTier": "default_claude_max_20x",
        }
    })
    .to_string()
}

/// A config dir holding `config`, with a stand-in `claude` built by `stub` that prints `stdout`.
fn home(config: &str, stdout: &str, stub: CliStub) -> (tempfile::TempDir, AgentCli) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".claude.json"), config).unwrap();
    std::fs::write(dir.path().join("report.jsonl"), stdout).unwrap();
    let cli = stub.stdout_file("report.jsonl").cli(dir.path());
    (dir, cli)
}

fn read(dir: &tempfile::TempDir, cli: &AgentCli) -> Result<ProviderLimitsDto, SavedReadError> {
    read_usage_within(
        &|| cli.command(),
        &dir.path().join(".claude.json"),
        &identity(),
        ANSWER,
    )
}

const ANSWER: Duration = crate::cli_stub::ANSWER_DEADLINE;

#[test]
fn a_report_reads_as_the_accounts_card_with_every_window_it_names() {
    let (dir, cli) = home(&config("user", "team"), REPORT, CliStub::new("claude"));

    let card = read(&dir, &cli).expect("a card");

    // Which windows, not their order: the card's order is `finish`'s.
    let mut windows: Vec<_> = card
        .reading
        .windows
        .iter()
        .map(|w| {
            (
                w.id.as_str(),
                w.kind,
                w.used_percent,
                w.resets_at.as_deref(),
            )
        })
        .collect();
    windows.sort_by_key(|window| window.0);
    assert_eq!(
        windows,
        [
            (
                "session",
                LimitWindowKind::Session,
                12.0,
                Some("2026-09-29T18:00:00.006917+00:00")
            ),
            (
                "weekly_all",
                LimitWindowKind::Weekly,
                34.0,
                Some("2026-10-05T09:00:00.006938+00:00")
            ),
            (
                "weekly_scoped:Fable",
                LimitWindowKind::Model,
                5.0,
                Some("2026-10-05T09:00:00+00:00")
            ),
        ]
    );
    let account = card.account.expect("the profile's account");
    assert_eq!(account.id, identity().observation_key());
    assert_eq!(account.label.as_deref(), Some("you@example.com"));
    assert!(!card.current_account);
    assert_eq!(card.status, LimitsStatus::Ok);
}

#[test]
fn the_plan_comes_from_the_organization_the_config_dir_names() {
    let (dir, cli) = home(&config("user", "team"), REPORT, CliStub::new("claude"));

    let card = read(&dir, &cli).expect("a card");

    assert_eq!(card.reading.plan.as_deref(), Some("max ×20"));
}

#[test]
fn claude_code_is_asked_for_its_usage_report_and_nothing_else() {
    let (dir, cli) = home(
        &config("user", "team"),
        REPORT,
        CliStub::new("claude").log_args("args.txt", false),
    );

    read(&dir, &cli).expect("a card");

    let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
    assert_eq!(
        args.trim(),
        "-p /usage --no-session-persistence --safe-mode --output-format stream-json --verbose"
    );
}

#[test]
fn claude_code_runs_without_updating_itself_and_without_an_inherited_credential() {
    let command = prepared(std::process::Command::new("claude"), &USAGE_ARGS);

    let envs: Vec<(String, Option<String>)> = command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect();
    assert!(envs.contains(&("DISABLE_AUTOUPDATER".into(), Some("1".into()))));
    // Any of these would make Claude Code read as that credential's account, not the config dir's.
    for name in [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN",
    ] {
        assert!(envs.contains(&(name.into(), None)), "{name} is inherited");
    }
}

#[test]
fn a_config_dir_naming_another_account_is_never_asked() {
    let (dir, cli) = home(
        &config("someone-else", "team"),
        REPORT,
        CliStub::new("claude").log_args("args.txt", false),
    );

    assert_eq!(read(&dir, &cli).unwrap_err(), SavedReadError::OtherAccount);
    assert!(!dir.path().join("args.txt").exists(), "claude was started");
}

#[test]
fn a_config_dir_naming_the_user_in_another_organization_is_never_asked() {
    // One user in two organizations is two accounts, each with its own card.
    let (dir, cli) = home(
        &config("user", "other-team"),
        REPORT,
        CliStub::new("claude").log_args("args.txt", false),
    );

    assert_eq!(read(&dir, &cli).unwrap_err(), SavedReadError::OtherAccount);
    assert!(!dir.path().join("args.txt").exists(), "claude was started");
}

#[test]
fn a_config_dir_naming_no_account_is_a_login_to_sign_in_again() {
    let (dir, cli) = home(
        "{}",
        REPORT,
        CliStub::new("claude").log_args("args.txt", false),
    );

    assert_eq!(
        read(&dir, &cli).unwrap_err(),
        SavedReadError::Http(HttpError::Unauthorized)
    );
    assert!(!dir.path().join("args.txt").exists(), "claude was started");
}

#[test]
fn a_report_after_which_the_config_dir_names_another_account_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".claude.json"), config("user", "team")).unwrap();
    std::fs::write(
        dir.path().join("other.json"),
        config("someone-else", "team"),
    )
    .unwrap();
    std::fs::write(dir.path().join("report.jsonl"), REPORT).unwrap();
    let cli = CliStub::new("claude")
        .copy("other.json", ".claude.json")
        .stdout_file("report.jsonl")
        .cli(dir.path());

    assert_eq!(read(&dir, &cli).unwrap_err(), SavedReadError::OtherAccount);
}

#[test]
fn output_without_a_usage_report_reads_as_no_card() {
    let output = "{\"type\":\"system\",\"subtype\":\"init\"}\nnot json\n{\"type\":\"result\",\"subtype\":\"success\"}";
    let (dir, cli) = home(&config("user", "team"), output, CliStub::new("claude"));

    assert!(matches!(
        read(&dir, &cli),
        Err(SavedReadError::Unavailable(
            "Claude Code reported no usage."
        ))
    ));
}

#[test]
fn a_report_of_windows_it_cannot_read_reads_as_no_card() {
    let output = r#"{"type":"assistant","usage_report":{"rate_limits":{"limits":[{"kind":"lunar","group":"lunar","percent":1}]}}}"#;
    let (dir, cli) = home(&config("user", "team"), output, CliStub::new("claude"));

    assert!(matches!(
        read(&dir, &cli),
        Err(SavedReadError::Unavailable(
            "Claude Code reported no usage."
        ))
    ));
}

#[test]
fn claude_code_failing_reads_as_unavailable_not_as_a_refused_login() {
    let (dir, cli) = home(
        &config("user", "team"),
        REPORT,
        CliStub::new("claude").exit(1),
    );

    assert!(matches!(
        read(&dir, &cli),
        Err(SavedReadError::Unavailable(
            "Claude Code could not report usage."
        ))
    ));
}

#[test]
fn a_claude_code_too_old_to_leave_customizations_out_says_to_update_it() {
    let (dir, cli) = home(
        &config("user", "team"),
        REPORT,
        CliStub::new("claude")
            .stderr("error: unknown option --safe-mode")
            .exit(1),
    );

    assert_eq!(
        read(&dir, &cli).unwrap_err(),
        SavedReadError::Unavailable(OUTDATED)
    );
}

#[test]
fn claude_code_that_does_not_answer_in_time_reads_as_unavailable() {
    let (dir, cli) = home(
        &config("user", "team"),
        REPORT,
        CliStub::new("claude").sleep(5),
    );

    let started = std::time::Instant::now();
    let result = read_usage_within(
        &|| cli.command(),
        &dir.path().join(".claude.json"),
        &identity(),
        Duration::from_millis(300),
    );

    assert!(matches!(
        result,
        Err(SavedReadError::Unavailable(
            "Claude Code could not report usage."
        ))
    ));
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "waited for the stub"
    );
}

/// A `claude` from `usage` for the first run, which asks for the report, and from `status` after,
/// which asks whether the config dir is signed in.
fn usage_then_status<'a>(
    usage: &'a AgentCli,
    status: &'a AgentCli,
) -> impl Fn() -> std::process::Command + 'a {
    let runs = std::cell::Cell::new(0);
    move || {
        runs.set(runs.get() + 1);
        if runs.get() == 1 {
            usage.command()
        } else {
            status.command()
        }
    }
}

#[test]
fn a_config_dir_claude_code_says_is_signed_out_is_a_login_to_sign_in_again() {
    let (dir, usage) = home(&config("user", "team"), "", CliStub::new("claude"));
    let status_dir = tempfile::tempdir().unwrap();
    // Claude Code 2.1.284 answers a signed-out `auth status` on stdout with exit status 1.
    let status = CliStub::new("claude")
        .log_args("args.txt", false)
        .stdout(r#"{"loggedIn":false,"authMethod":"none"}"#)
        .exit(1)
        .cli(status_dir.path());

    let result = read_usage_within(
        &usage_then_status(&usage, &status),
        &dir.path().join(".claude.json"),
        &identity(),
        ANSWER,
    );

    assert_eq!(
        result.unwrap_err(),
        SavedReadError::Http(HttpError::Unauthorized)
    );
    let args = std::fs::read_to_string(status_dir.path().join("args.txt")).unwrap();
    assert_eq!(args.trim(), "auth status --json");
}

#[test]
fn no_report_from_a_config_dir_still_signed_in_reads_as_unavailable() {
    let (dir, usage) = home(&config("user", "team"), "", CliStub::new("claude"));
    let status_dir = tempfile::tempdir().unwrap();
    let status = CliStub::new("claude")
        .stdout(r#"{"loggedIn":true,"authMethod":"claude.ai"}"#)
        .cli(status_dir.path());

    let result = read_usage_within(
        &usage_then_status(&usage, &status),
        &dir.path().join(".claude.json"),
        &identity(),
        ANSWER,
    );

    assert!(matches!(
        result,
        Err(SavedReadError::Unavailable(
            "Claude Code reported no usage."
        ))
    ));
}

#[test]
fn no_report_and_a_status_claude_code_cannot_give_reads_as_unavailable() {
    let (dir, usage) = home(&config("user", "team"), "", CliStub::new("claude"));
    let status_dir = tempfile::tempdir().unwrap();
    let status = CliStub::new("claude")
        .stdout("not a status")
        .exit(1)
        .cli(status_dir.path());

    let result = read_usage_within(
        &usage_then_status(&usage, &status),
        &dir.path().join(".claude.json"),
        &identity(),
        ANSWER,
    );

    assert!(matches!(
        result,
        Err(SavedReadError::Unavailable(
            "Claude Code reported no usage."
        ))
    ));
}

mod signed_in;
