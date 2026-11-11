use super::*;
use crate::cli_stub::CliStub;

const STORED: i64 = 1_791_891_940_000;
const REPORTED: i64 = 1_791_822_700_250;
const DATED: &str = r#"{"loggedIn":true,"authMethod":"claude.ai","email":"a@example.com","orgId":"org-a","loginExpiresAt":"2026-10-12T16:31:40.250Z"}"#;
const UNDATED: &str = r#"{"loggedIn":true,"authMethod":"claude.ai","email":"a@example.com","orgId":"org-a","loginExpiresAt":null}"#;
const NO_LOGIN: &str = r#"{"loggedIn":false,"authMethod":"none","loginExpiresAt":null}"#;

fn account() -> Value {
    json!({"accountUuid":"user-a","organizationUuid":"org-a","emailAddress":"a@example.com"})
}

fn login() -> Value {
    json!({"claudeAiOauth":{
        "accessToken":"access","refreshToken":"refresh",
        "expiresAt":1_791_850_000_000_i64,"refreshTokenExpiresAt":STORED
    }})
}

fn stored(root: &Path, credentials: &Value, config: &Value) -> ClaudeNative {
    let native = claude(root);
    fs::write(
        native.config_home.join(".credentials.json"),
        credentials.to_string(),
    )
    .unwrap();
    fs::write(&native.config_file, config.to_string()).unwrap();
    native
}

fn signed_in(root: &Path) -> ClaudeNative {
    stored(root, &login(), &json!({"oauthAccount": account()}))
}

fn ending(ends_at_ms: Option<i64>) -> Option<SignInDeadline> {
    Some(SignInDeadline {
        identity: Identity {
            provider: AgentId::Claude,
            user_id: "user-a".into(),
            workspace_id: "org-a".into(),
        },
        ends_at_ms,
    })
}

fn deadline(at: i64) -> Option<SignInDeadline> {
    ending(Some(at))
}

fn answering(root: &Path, claude_code: CliStub) -> PathBuf {
    claude_code
        .log_args("asked.txt", true)
        .log_env("CLAUDE_CONFIG_DIR", "config-dir.txt")
        .write(&root.join("bin"))
}

fn told(root: &Path, native: &ClaudeNative, status: &str) -> Option<SignInDeadline> {
    let stub = answering(root, CliStub::new("claude").stdout(status));
    crate::accounts::native::with_test_cli(&stub, || {
        native.sign_in_deadline_within(crate::cli_stub::ANSWER_DEADLINE)
    })
}

fn asked(root: &Path) -> Vec<String> {
    fs::read_to_string(root.join("bin").join("asked.txt"))
        .unwrap_or_default()
        .lines()
        .map(|line| line.trim().to_string())
        .collect()
}

#[test]
fn the_sign_in_deadline_is_the_date_claude_code_reports_for_its_login() {
    let root = tempfile::tempdir().unwrap();
    let native = signed_in(root.path());

    let found = told(root.path(), &native, DATED);

    assert_eq!(found, deadline(REPORTED));
    assert_eq!(asked(root.path()), ["auth status --json"]);
    let config_dir = fs::read_to_string(root.path().join("bin").join("config-dir.txt")).unwrap();
    assert_eq!(Path::new(config_dir.trim()), native.config_home.as_path());
}

#[test]
fn a_claude_ai_login_claude_code_dates_no_end_for_is_this_accounts_with_no_date() {
    let root = tempfile::tempdir().unwrap();
    let native = signed_in(root.path());

    assert_eq!(told(root.path(), &native, UNDATED), ending(None));
}

#[test]
fn an_answer_for_another_account_is_not_this_accounts() {
    for (status, why) in [
        (
            DATED.replace("org-a", "org-b"),
            "another organization's date",
        ),
        (
            DATED.replace("a@example.com", "b@example.com"),
            "another email's date",
        ),
        (
            UNDATED.replace("org-a", "org-b"),
            "another organization's lack of one",
        ),
        (
            DATED.replace(r#""orgId":"org-a","#, ""),
            "a date for no organization",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let native = signed_in(root.path());

        assert_eq!(told(root.path(), &native, &status), None, "{why}");
    }
}

#[test]
fn an_account_record_without_an_email_takes_the_date_for_its_organization() {
    let root = tempfile::tempdir().unwrap();
    let native = stored(
        root.path(),
        &login(),
        &json!({"oauthAccount":{"accountUuid":"user-a","organizationUuid":"org-a"}}),
    );

    assert_eq!(told(root.path(), &native, DATED), deadline(REPORTED));
}

#[test]
fn the_date_stored_with_the_login_stands_where_claude_code_reports_none_for_it() {
    for (status, why) in [
        (
            r#"{"loggedIn":true,"authMethod":"claude.ai","email":"a@example.com","orgId":"org-a"}"#,
            "a Claude Code from before it reported the date",
        ),
        (
            r#"{"loggedIn":true,"authMethod":"api_key_helper","loginExpiresAt":null}"#,
            "signed in another way than the stored login",
        ),
        (
            r#"{"loggedIn":true,"authMethod":"claude.ai","loginExpiresAt":"soon"}"#,
            "a date that cannot be read",
        ),
        (
            r#"{"loggedIn":true,"authMethod":"claude.ai","loginExpiresAt":1791822700250}"#,
            "a date that is not text",
        ),
        ("not a status", "an answer that is not a status"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let native = signed_in(root.path());

        assert_eq!(
            told(root.path(), &native, status),
            deadline(STORED),
            "{why}"
        );
        assert_eq!(asked(root.path()), ["auth status --json"], "{why}");
    }
}

#[test]
fn the_stored_date_stands_when_claude_code_cannot_be_asked() {
    let root = tempfile::tempdir().unwrap();
    let native = signed_in(root.path());
    let failing = answering(root.path(), CliStub::new("claude").exit(1));
    let absent = root.path().join("bin").join("no-such-claude");

    for (cli, why) in [(failing, "it fails without a word"), (absent, "no CLI")] {
        let found = crate::accounts::native::with_test_cli(&cli, || {
            NativeAccount::sign_in_deadline(&native)
        });
        assert_eq!(found, deadline(STORED), "{why}");
    }
}

#[test]
fn a_claude_code_that_takes_too_long_leaves_the_stored_date() {
    let root = tempfile::tempdir().unwrap();
    let native = signed_in(root.path());
    let slow = answering(root.path(), CliStub::new("claude").stdout(DATED).sleep(5));

    let started = std::time::Instant::now();
    let found = crate::accounts::native::with_test_cli(&slow, || {
        native.sign_in_deadline_within(Duration::from_millis(300))
    });

    assert_eq!(found, deadline(STORED));
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "the listing does not wait for it"
    );
}

#[test]
fn a_login_claude_code_gave_up_on_keeps_the_deadline_it_ended_at() {
    let root = tempfile::tempdir().unwrap();
    let native = stored(
        root.path(),
        &json!({"claudeAiOauth":{
            "accessToken":"","refreshToken":"","expiresAt":0,
            "refreshTokenExpiresAt":STORED,"scopes":["user:inference"],"subscriptionType":"max"
        }}),
        &json!({"oauthAccount": account()}),
    );

    assert!(native.read().unwrap().is_none(), "no usable login is left");
    assert_eq!(told(root.path(), &native, NO_LOGIN), deadline(STORED));
}

#[test]
fn a_store_with_no_account_record_has_no_deadline_and_claude_code_is_not_asked() {
    let root = tempfile::tempdir().unwrap();
    let native = stored(root.path(), &login(), &json!({}));

    assert_eq!(told(root.path(), &native, DATED), None);
    assert_eq!(asked(root.path()), Vec::<String>::new());

    let unwritten = tempfile::tempdir().unwrap();
    assert_eq!(
        told(unwritten.path(), &claude(unwritten.path()), DATED),
        None,
        "no store"
    );
    assert_eq!(asked(unwritten.path()), Vec::<String>::new());
}

#[test]
fn an_account_record_without_its_organization_has_no_deadline_and_claude_code_is_not_asked() {
    let root = tempfile::tempdir().unwrap();
    let native = stored(
        root.path(),
        &login(),
        &json!({"oauthAccount":{"accountUuid":"user-a"}}),
    );

    assert_eq!(told(root.path(), &native, DATED), None);
    assert_eq!(asked(root.path()), Vec::<String>::new());
}

#[test]
fn there_is_no_deadline_without_a_reported_or_stored_date_and_the_account_it_is_for() {
    let dated = login();
    for (credentials, config, why) in [
        (
            serde_json::from_str(SIGNED_OUT).unwrap(),
            json!({"oauthAccount": account()}),
            "signed out by on-n-off",
        ),
        (
            json!({"claudeAiOauth":{"accessToken":"a","refreshToken":"r"}}),
            json!({"oauthAccount": account()}),
            "no date stored",
        ),
        (
            json!({"claudeAiOauth":{"accessToken":"a","refreshToken":"r","refreshTokenExpiresAt":"soon"}}),
            json!({"oauthAccount": account()}),
            "a stored date that is not a number",
        ),
        (
            dated,
            json!({"oauthAccount":{"accountUuid":"user-a"}}),
            "an account without its organization",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let native = stored(root.path(), &credentials, &config);
        assert_eq!(told(root.path(), &native, NO_LOGIN), None, "{why}");
    }
}

#[test]
fn the_listing_gives_claude_code_time_to_answer_without_waiting_long_for_it() {
    assert!(
        LOGIN_END_PATIENCE >= Duration::from_secs(1),
        "too short for Claude Code to start and answer"
    );
    assert!(
        LOGIN_END_PATIENCE <= Duration::from_secs(10),
        "the account listing waits for this"
    );
}

#[test]
#[ignore = "asks the Claude Code signed in at this process's own home, named in ON_N_OFF_PROBE_HOME, when its sign-in ends"]
fn probe_real_home_sign_in_deadline() {
    #[cfg(target_os = "macos")]
    crate::accounts::with_real_keychain(print_real_home_sign_in_deadline);
    #[cfg(not(target_os = "macos"))]
    print_real_home_sign_in_deadline();
}

fn print_real_home_sign_in_deadline() {
    let home = crate::paths::probe_home();
    let native = ClaudeNative::resolve_from(&home, &|_| None).unwrap();
    let status = crate::limits::claude_cli::auth_status(&|| native.command());
    let reported = status.as_ref().and_then(ReportedEnd::of);
    let deadline = NativeAccount::sign_in_deadline(&native);
    let dated = matches!(reported, Some(ReportedEnd(Some(_))));
    let same = match (&reported, &deadline) {
        (Some(ReportedEnd(at)), Some(deadline)) => dated && deadline.ends_at_ms == *at,
        _ => false,
    };
    println!("Claude Code answered: {}", status.is_some());
    println!("Claude Code dates the sign-in's end: {dated}");
    println!("deadline read: {}", deadline.is_some());
    println!("deadline is the date Claude Code reports: {same}");
}
