use super::mint::fixtures::{mint_body, mint_response, scoped_you};
use super::*;
use crate::dto::{LimitWindowKind, LimitsStatus};
use crate::http::RateLimitReset;
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
    .expect("mint reading");
    assert_eq!(read.reading.plan.as_deref(), Some("Wonder"));
}

#[test]
fn no_login_reads_signed_out_or_not_installed_without_asking_meta() {
    let root = crate::paths::scratch_dir("muse-no-login");
    for (installed, expected) in [
        (true, "Sign in with `muse` to see subscription limits."),
        (
            false,
            "Install Muse Code and sign in with `muse` to see subscription limits.",
        ),
    ] {
        let card = finish_read(read_current(
            &root,
            || installed,
            |_| None,
            mint_must_not_run,
        ));
        assert_eq!(card.status, LimitsStatus::SignedOut, "{expected}");
        assert_eq!(card.message.as_deref(), Some(expected));
        assert!(card.account.is_none(), "{expected}");
    }
}

#[test]
fn the_current_card_without_a_login_names_whether_the_muse_cli_was_found() {
    let card = muse_current(&crate::paths::scratch_dir("muse-current-no-login"));
    let expected = if resolve_provider_cli(AgentId::Muse, "muse").is_some() {
        "Sign in with `muse` to see subscription limits."
    } else {
        "Install Muse Code and sign in with `muse` to see subscription limits."
    };
    assert_eq!(card.status, LimitsStatus::SignedOut);
    assert_eq!(card.message.as_deref(), Some(expected));
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
    let card = finish_read(read_current(
        &root,
        installed,
        |_| login(Some("you@example.com"), MuseCredential::ApiKey),
        mint_must_not_run,
    ));
    assert_eq!(card.status, LimitsStatus::Unsupported);
    assert_eq!(
        card.message.as_deref(),
        Some("Muse is signed in with an API key, which has no subscription limits.")
    );
}

#[test]
fn an_expired_or_unread_login_fails_the_card_under_its_account_without_asking_meta() {
    let root = crate::paths::scratch_dir("muse-login-unusable");
    for (credential, expected) in [
        (
            MuseCredential::Expired,
            "Muse's login has expired. Sign in with `muse` again to see subscription limits.",
        ),
        (
            MuseCredential::Unread,
            "on-n-off could not read Muse's login, so it did not ask Meta for usage. It reads the login again when Muse's login changes or after on-n-off restarts.",
        ),
    ] {
        let card = finish_read(read_current(
            &root,
            installed,
            |_| login(Some("You@Example.com"), credential),
            mint_must_not_run,
        ));
        assert_eq!(card.status, LimitsStatus::Failed, "{expected}");
        assert_eq!(card.message.as_deref(), Some(expected));
        assert_eq!(
            card.account.as_ref().map(|account| account.id.as_str()),
            Some(scoped_you().as_str()),
            "{expected}"
        );
        assert!(card.reading.windows.is_empty(), "{expected}");
    }
}

#[test]
fn a_mint_miss_fails_the_card_under_its_account() {
    let root = crate::paths::scratch_dir("muse-mint-miss");
    let unanswered = |why: &str| {
        format!("The Muse usage read failed ({why}). on-n-off will read it again later.")
    };
    let misses: [(Result<mint::MintResponse, HttpError>, String); 6] = [
        (
            Err(HttpError::Unauthorized),
            "Meta did not accept Muse's login. Sign in with `muse` again to see subscription limits."
                .to_string(),
        ),
        (
            Err(HttpError::RateLimited(RateLimitReset::Unknown)),
            "Meta is limiting how often Muse usage can be read. on-n-off will read it again later."
                .to_string(),
        ),
        (
            Err(HttpError::Network("connection refused".to_string())),
            unanswered("network error: connection refused"),
        ),
        (Err(HttpError::Status(503)), unanswered("HTTP 503")),
        (
            Err(HttpError::Parse("Unreadable Muse key answer.".to_string())),
            unanswered("unreadable response: Unreadable Muse key answer."),
        ),
        (
            Ok(mint_response(json!({"subs_usage": null}))),
            "Meta's answer had no subscription usage for this Muse account.".to_string(),
        ),
    ];
    for (answer, expected) in misses {
        let card = finish_read(read_current(
            &root,
            installed,
            |_| signed_in(Some("You@Example.com")),
            |_| answer,
        ));
        assert_eq!(card.status, LimitsStatus::Failed, "{expected}");
        assert_eq!(card.message.as_deref(), Some(expected.as_str()));
        assert_eq!(
            card.account.as_ref().map(|account| account.id.as_str()),
            Some(scoped_you().as_str()),
            "{expected}"
        );
        assert!(card.reading.windows.is_empty(), "{expected}");
    }
}

#[test]
fn an_inactive_or_unpaid_answer_reads_unsupported() {
    let root = crate::paths::scratch_dir("muse-mint-unsupported");
    for (body, expected) in [
        (
            json!({"is_subs_active": false, "subs_tier_name": "Wonder"}),
            "This Muse subscription is inactive, so it has no subscription limits to show.",
        ),
        (
            json!({"is_subs_active": true, "require_payment": true}),
            "Muse subscription billing is incomplete; finish it in Muse to see subscription limits.",
        ),
    ] {
        let card = finish_read(read_current(
            &root,
            installed,
            |_| signed_in(Some("You@Example.com")),
            |_| Ok(mint_response(body)),
        ));
        assert_eq!(card.status, LimitsStatus::Unsupported, "{expected}");
        assert_eq!(card.message.as_deref(), Some(expected));
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

fn failed_account(read: Result<Parsed, ReadFailure>) -> LimitsAccountDto {
    match read {
        Err(ReadFailure::Failed(account, _)) => account,
        other => panic!("expected a failed card, got {other:?}"),
    }
}

fn every_failing_path(root: &Path, email: Option<&str>) -> Vec<LimitsAccountDto> {
    let missed = read_current(
        root,
        installed,
        |_| signed_in(email),
        |_| Err(HttpError::Status(500)),
    );
    let mut accounts = vec![failed_account(missed)];
    for credential in [MuseCredential::Expired, MuseCredential::Unread] {
        accounts.push(failed_account(read_current(
            root,
            installed,
            |_| login(email, credential),
            mint_must_not_run,
        )));
    }
    accounts
}

#[test]
fn every_path_names_the_account_as_the_mint_reading_does() {
    let root = crate::paths::scratch_dir("muse-identity");
    let minted = read_current(
        &root,
        installed,
        |_| signed_in(Some("You@Example.com")),
        |_| Ok(mint_response(mint_body())),
    )
    .expect("mint reading")
    .account
    .expect("account");
    assert_eq!(minted.id, scoped_you());
    assert_eq!(minted.legacy_id.as_deref(), Some("email:you@example.com"));
    assert_eq!(minted.label.as_deref(), Some("You@Example.com"));
    for account in every_failing_path(&root, Some("You@Example.com")) {
        assert_eq!(account, minted);
    }
}

#[test]
fn a_login_without_an_email_is_the_default_account_on_every_path() {
    let root = crate::paths::scratch_dir("muse-identity-default");
    let minted = read_current(
        &root,
        installed,
        |_| signed_in(None),
        |_| Ok(mint_response(mint_body())),
    )
    .expect("mint reading")
    .account
    .expect("account");
    assert_eq!(minted.id, super::super::DEFAULT_ACCOUNT);
    for account in every_failing_path(&root, None) {
        assert_eq!(account, minted);
    }
}
