use super::mint::fixtures::{mint_body, mint_response, scoped_you};
use super::*;
use crate::dto::LimitWindowKind;
use crate::http::RateLimitReset;
use crate::limits::outcome::tests::every_failure_keeps_the_account_its_answer_names;
use serde_json::json;

fn login(email: Option<&str>, credential: MuseCredential) -> Option<MuseLogin> {
    Some(MuseLogin {
        email: email.map(str::to_string),
        credential,
    })
}

fn signed_in(email: Option<&str>) -> Option<MuseLogin> {
    login(
        email,
        MuseCredential::Token(AccessToken::new("device-token")),
    )
}

fn installed() -> bool {
    true
}

fn mint_must_not_run(_: &AccessToken) -> Result<mint::MintResponse, HttpError> {
    panic!("Meta must not be asked")
}

fn known_id(read: &Read) -> Option<&str> {
    read.account.as_ref().map(|account| account.id.as_str())
}

#[test]
fn a_token_reads_the_card_from_the_mint() {
    let root = crate::paths::scratch_dir("muse-mint-reading");
    let config = crate::paths::muse_config_for(&root);
    let read = read_current(
        &root,
        installed,
        |asked: &Path| {
            assert_eq!(asked, config);
            signed_in(Some("you@example.com"))
        },
        |token| {
            assert_eq!(token.authorization(), "Bearer device-token");
            Ok(mint_response(mint_body()))
        },
    )
    .answer
    .expect("mint reading");
    assert_eq!(read.reading.plan.as_deref(), Some("Wonder"));
}

#[test]
fn no_login_reads_signed_out_or_not_installed_without_asking_meta() {
    let root = crate::paths::scratch_dir("muse-no-login");
    for (installed, expected) in [
        (true, ReadFailure::SignedOut),
        (false, ReadFailure::NotInstalled),
    ] {
        let read = read_current(&root, || installed, |_| None, mint_must_not_run);
        assert_eq!(read.answer.err(), Some(expected.clone()));
        assert!(read.account.is_none(), "{expected:?}");
    }
}

#[test]
fn the_current_card_without_a_login_names_whether_the_muse_cli_was_found() {
    let read = muse_current(&crate::paths::scratch_dir("muse-current-no-login"));
    let expected = if resolve_provider_cli(AgentId::Muse, "muse").is_some() {
        ReadFailure::SignedOut
    } else {
        ReadFailure::NotInstalled
    };
    assert_eq!(read.answer.err(), Some(expected));
}

#[test]
fn a_login_never_asks_whether_muse_is_installed() {
    let root = crate::paths::scratch_dir("muse-login-installed");
    for credential in [
        MuseCredential::Token(AccessToken::new("device-token")),
        MuseCredential::ApiKey,
        MuseCredential::Expired,
        MuseCredential::Unread,
    ] {
        let _ = read_current(
            &root,
            || panic!("a login card never depends on the Muse CLI"),
            |_| login(Some("you@example.com"), credential),
            |_| Ok(mint_response(mint_body())),
        );
    }
}

#[test]
fn an_api_key_login_reads_unsupported_without_asking_meta() {
    let root = crate::paths::scratch_dir("muse-api-key");
    let read = read_current(
        &root,
        installed,
        |_| login(Some("you@example.com"), MuseCredential::ApiKey),
        mint_must_not_run,
    );
    assert_eq!(
        read.answer.err(),
        Some(ReadFailure::Unsupported(Unsupported::ApiKey))
    );
}

#[test]
fn an_expired_login_is_refused_and_an_unread_one_unavailable_without_asking_meta() {
    let root = crate::paths::scratch_dir("muse-login-unusable");
    for (credential, expected) in [
        (MuseCredential::Expired, ReadFailure::LoginRefused),
        (
            MuseCredential::Unread,
            ReadFailure::Unavailable(UNREAD.to_string()),
        ),
    ] {
        let read = read_current(
            &root,
            installed,
            |_| login(Some("You@Example.com"), credential),
            mint_must_not_run,
        );
        assert_eq!(known_id(&read), Some(scoped_you().as_str()));
        assert_eq!(read.answer.err(), Some(expected));
    }
}

#[test]
fn a_mint_miss_is_the_failure_of_its_call() {
    let root = crate::paths::scratch_dir("muse-mint-miss");
    for error in [
        HttpError::Unauthorized,
        HttpError::RateLimited(RateLimitReset::RetryAfter(30)),
        HttpError::Network("connection refused".to_string()),
        HttpError::Status(503),
        HttpError::Parse("Unreadable Muse key answer.".to_string()),
    ] {
        let read = read_current(
            &root,
            installed,
            |_| signed_in(Some("You@Example.com")),
            |_| Err(error.clone()),
        );
        assert_eq!(known_id(&read), Some(scoped_you().as_str()));
        assert_eq!(read.answer.err(), Some(ReadFailure::from(error)));
    }
}

#[test]
fn an_answer_without_usage_reads_no_window() {
    let read = read_current(
        &crate::paths::scratch_dir("muse-mint-no-usage"),
        installed,
        |_| signed_in(Some("You@Example.com")),
        |_| Ok(mint_response(json!({"subs_usage": null}))),
    );
    assert!(read.answer.expect("an answer").reading.windows.is_empty());
}

#[test]
fn an_inactive_or_unpaid_answer_reads_unsupported() {
    let root = crate::paths::scratch_dir("muse-mint-unsupported");
    for (body, expected) in [
        (
            json!({"is_subs_active": false, "subs_tier_name": "Wonder"}),
            Unsupported::Inactive,
        ),
        (
            json!({"is_subs_active": true, "require_payment": true}),
            Unsupported::BillingIncomplete,
        ),
    ] {
        let read = read_current(
            &root,
            installed,
            |_| signed_in(Some("You@Example.com")),
            |_| Ok(mint_response(body)),
        );
        assert_eq!(read.answer.err(), Some(ReadFailure::Unsupported(expected)));
    }
}

#[test]
fn an_idle_mint_answer_reads_ok() {
    let root = crate::paths::scratch_dir("muse-mint-idle");
    let read = read_current(
        &root,
        installed,
        |_| signed_in(Some("you@example.com")),
        |_| {
            Ok(mint_response(json!({
                "is_subs_active": true,
                "subs_tier_name": "Muse Code Power Usage",
                "subs_usage": null
            })))
        },
    )
    .answer
    .expect("idle mint reading");
    assert_eq!(read.reading.plan.as_deref(), Some("Muse Code Power Usage"));
    assert_eq!(
        read.reading
            .windows
            .iter()
            .map(|window| (window.kind, window.used_percent))
            .collect::<Vec<_>>(),
        [(LimitWindowKind::Session, 0.0)]
    );
}

fn minted(root: &Path, email: Option<&str>) -> Read {
    read_current(
        root,
        installed,
        |_| signed_in(email),
        |_| Ok(mint_response(mint_body())),
    )
}

fn every_failing_path(root: &Path, email: Option<&str>) -> Vec<Read> {
    let mut reads = vec![
        read_current(
            root,
            installed,
            |_| signed_in(email),
            |_| Err(HttpError::Status(500)),
        ),
        read_current(
            root,
            installed,
            |_| signed_in(email),
            |_| Ok(mint_response(json!({"subs_usage": null}))),
        ),
        read_current(
            root,
            installed,
            |_| signed_in(email),
            |_| Ok(mint_response(json!({"is_subs_active": false}))),
        ),
    ];
    for credential in [
        MuseCredential::Expired,
        MuseCredential::Unread,
        MuseCredential::ApiKey,
    ] {
        reads.push(read_current(
            root,
            installed,
            |_| login(email, credential),
            mint_must_not_run,
        ));
    }
    reads
}

#[test]
fn every_failure_keeps_the_account_the_mint_reading_names() {
    let root = crate::paths::scratch_dir("muse-identity");
    let minted_account = minted(&root, Some("You@Example.com"))
        .answer
        .expect("mint reading")
        .account
        .expect("account");
    assert_eq!(minted_account.id, scoped_you());
    assert_eq!(
        minted_account.legacy_id.as_deref(),
        Some("email:you@example.com")
    );
    assert_eq!(minted_account.label.as_deref(), Some("You@Example.com"));
    every_failure_keeps_the_account_its_answer_names(
        AgentId::Muse,
        minted(&root, Some("You@Example.com")),
        every_failing_path(&root, Some("You@Example.com")),
    );
}

#[test]
fn a_login_without_an_email_is_the_default_account_on_every_path() {
    let root = crate::paths::scratch_dir("muse-identity-default");
    let minted_account = minted(&root, None)
        .answer
        .expect("mint reading")
        .account
        .expect("account");
    assert_eq!(minted_account.id, super::super::DEFAULT_ACCOUNT);
    every_failure_keeps_the_account_its_answer_names(
        AgentId::Muse,
        minted(&root, None),
        every_failing_path(&root, None),
    );
}

#[test]
fn the_native_login_is_the_account_a_read_records_without_asking_the_keychain() {
    use crate::accounts::muse_store::LoginCache;
    let bundle = r#"{"secret_schema_version":1,"access_token":"keychain-token"}"#;
    for (auth, expected) in [
        (
            Some(
                r#"{"providers":{"meta":{"mechanism":"oauth","access_token":"device-token","user_email":"You@Example.com"}}}"#,
            ),
            Some(scoped_you()),
        ),
        (
            Some(
                r#"{"providers":{"meta":{"mechanism":"oauth","storage":"keychain","user_email":"you@example.com"}}}"#,
            ),
            Some(scoped_you()),
        ),
        (
            Some(r#"{"providers":{"meta":{"mechanism":"oauth","access_token":"device-token"}}}"#),
            Some(super::super::DEFAULT_ACCOUNT.to_string()),
        ),
        (None, None),
    ] {
        let home = crate::paths::scratch_dir("muse-native-login");
        let config = crate::paths::muse_config_for(&home);
        if let Some(auth) = auth {
            std::fs::create_dir_all(&config).unwrap();
            std::fs::write(config.join("auth.json"), auth).unwrap();
        }
        let read = read_current(
            &home,
            installed,
            |config| LoginCache::new().login(config, 0, &|| Ok(Some(bundle.to_string()))),
            |_| Err(HttpError::Status(503)),
        );
        let (_, outcome) = crate::limits::outcome::signed_in(AgentId::Muse, read);

        assert_eq!(outcome.login, expected, "{auth:?}");
        assert_eq!(
            crate::limits::native_login_at(AgentId::Muse, &home),
            outcome.login,
            "{auth:?}"
        );
    }
}
