//! The first usage reading of an isolated Claude sign-in (`IsolatedSignIn::first_usage`): Claude
//! Code's own report, asked in the sign-in's config dir about the account that dir names.
use super::*;
use crate::cli_stub::CliStub;

const REPORT: &str = r#"{"type":"assistant","usage_report":{"rate_limits":{"limits":[{"kind":"weekly_all","group":"weekly","percent":34,"resets_at":"2026-10-05T09:00:00+00:00"}]}}}"#;

fn identity() -> Identity {
    Identity {
        provider: AgentId::Claude,
        user_id: "user".into(),
        workspace_id: "team".into(),
    }
}

/// An isolated sign-in under `root` whose config dir names `user` in `org`, and a stand-in
/// `claude` beside it that prints `REPORT` and records the config dir it was given.
fn signed_in(root: &Path, user: &str, org: &str) -> (ClaudeNative, PathBuf) {
    let native = ClaudeNative::isolated(root).unwrap();
    let account = json!({"oauthAccount": {
        "accountUuid": user,
        "organizationUuid": org,
        "emailAddress": "you@example.com",
    }});
    fs::write(&native.config_file, account.to_string()).unwrap();
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(bin.join("report.jsonl"), REPORT).unwrap();
    let stub = CliStub::new("claude")
        .log_env("CLAUDE_CONFIG_DIR", "config-dir.txt")
        .stdout_file("report.jsonl")
        .write(&bin);
    (native, stub)
}

#[test]
fn a_sign_ins_first_usage_is_claude_codes_report_in_its_config_dir() {
    let root = tempfile::tempdir().unwrap();
    let (native, stub) = signed_in(root.path(), "user", "team");

    let card = crate::accounts::native::with_test_cli(&stub, || {
        native.first_usage(root.path(), &identity())
    })
    .expect("a card");

    assert_eq!(
        card.account.map(|a| a.id),
        Some(identity().observation_key())
    );
    let windows: Vec<_> = card
        .reading
        .windows
        .iter()
        .map(|w| (w.id.as_str(), w.used_percent))
        .collect();
    assert_eq!(windows, [("weekly_all", 34.0)]);
    // Claude Code ran with the sign-in's own config dir, not whichever account the app's home has.
    let config_dir = fs::read_to_string(root.path().join("bin").join("config-dir.txt")).unwrap();
    assert_eq!(Path::new(config_dir.trim()), native.config_home.as_path());
}

#[test]
fn a_sign_in_whose_config_dir_names_another_account_has_no_first_usage() {
    let root = tempfile::tempdir().unwrap();
    let (native, stub) = signed_in(root.path(), "user", "other-team");

    let card = crate::accounts::native::with_test_cli(&stub, || {
        native.first_usage(root.path(), &identity())
    });

    assert!(card.is_none());
}
