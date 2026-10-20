use super::*;
use crate::limits::outcome::tests::{every_failure_keeps_the_account_its_answer_names, Expected};

fn write_login(home: &Path, user: &str, workspace: &str) {
    let id_token = crate::accounts::codex::tests::id_token(
        &json!({"chatgpt_user_id": user, "chatgpt_account_id": workspace}),
    );
    std::fs::write(
        home.join(".codex/auth.json"),
        json!({"tokens": {"account_id": workspace, "access_token": "fixture-access", "id_token": id_token}})
            .to_string(),
    )
    .unwrap();
}

fn signed_in_home(name: &str) -> PathBuf {
    let home = crate::paths::scratch_dir(name);
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    write_login(&home, "user-1", "workspace-1");
    home
}

fn answering(codex_home: &Path, kind: &str, rate_limits: Value) -> Session {
    Script::new([
        json!({"id": 1, "result": {"codexHome": codex_home.to_string_lossy()}}),
        json!({"id": 2, "result": {"account": {"type": kind, "email": "you@example.com", "planType": "pro"}}}),
        json!({"id": 3, "result": {"rateLimits": rate_limits}}),
    ])
    .session(APP_SERVER)
}

fn weekly() -> Value {
    json!({"limitId": "codex", "primary": {"usedPercent": 12, "windowDurationMins": 10080}})
}

fn read(home: &Path, start: impl FnOnce(&Path) -> Result<Session, ReadFailure>) -> Read {
    read_with(
        home,
        false,
        start,
        |_: &mut Parsed, _: Option<&CodexAccess>| {},
    )
}

#[test]
fn every_failure_after_the_login_is_read_keeps_its_account() {
    let home = signed_in_home("codex-known-account");
    let codex_home = home.join(".codex");
    let answered = read(&home, |_| Ok(answering(&codex_home, "chatgpt", weekly())));
    let failing: Vec<(Read, Expected)> = vec![
        (
            read(&home, |_| {
                Err(ReadFailure::Unavailable(
                    "Codex app-server exited.".to_string(),
                ))
            }),
            |failure| *failure == ReadFailure::Unavailable("Codex app-server exited.".into()),
        ),
        (
            read(&home, |_| {
                Ok(
                    Script::new([json!({"id": 1, "result": {"codexHome": codex_home}})])
                        .session(APP_SERVER),
                )
            }),
            |failure| matches!(failure, ReadFailure::Unavailable(_)),
        ),
        (
            read(&home, |_| Ok(answering(&codex_home, "apiKey", weekly()))),
            |failure| *failure == ReadFailure::Unsupported(Unsupported::ApiKey),
        ),
        (
            read(&home, |_| Ok(answering(&codex_home, "chatgpt", json!({})))),
            |failure| *failure == ReadFailure::NoUsage,
        ),
        (
            read(&home, |_| {
                write_login(&home, "user-2", "workspace-1");
                Ok(answering(&codex_home, "chatgpt", weekly()))
            }),
            |failure| *failure == ReadFailure::AccountChanged,
        ),
    ];

    every_failure_keeps_the_account_its_answer_names(AgentId::Codex, answered, failing);
}

#[test]
fn without_a_login_nobody_is_named() {
    let home = crate::paths::scratch_dir("codex-no-login");
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    let codex_home = home.join(".codex");

    let read = read(&home, |_| {
        Ok(Script::new([
            json!({"id": 1, "result": {"codexHome": codex_home.to_string_lossy()}}),
            json!({"id": 2, "result": {"account": null}}),
            json!({"id": 3, "result": {"rateLimits": {}}}),
        ])
        .session(APP_SERVER))
    });

    assert!(read.account.is_none());
    assert_eq!(read.answer.err(), Some(ReadFailure::SignedOut));
}

#[test]
fn the_native_login_is_the_account_a_read_records() {
    let signed_in_home = signed_in_home("codex-native-login");
    let signed_out_home = crate::paths::scratch_dir("codex-native-login-none");
    std::fs::create_dir_all(signed_out_home.join(".codex")).unwrap();
    for (home, signed_in) in [(&signed_in_home, true), (&signed_out_home, false)] {
        let (_, outcome) = crate::limits::outcome::signed_in(
            AgentId::Codex,
            read(home, |_| Err(ReadFailure::NotInstalled)),
        );

        assert_eq!(outcome.login.is_some(), signed_in);
        assert_eq!(
            crate::limits::native_login_at(AgentId::Codex, home),
            outcome.login,
            "signed in: {signed_in}"
        );
    }
}
