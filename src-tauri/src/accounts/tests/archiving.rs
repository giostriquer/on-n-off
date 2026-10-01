use super::fixture::{claude, codex, identity, Harness};
use crate::accounts::{model::Identity, store::Login};
use crate::dto::{AgentId, LimitWindowDto, LimitWindowKind, ProviderLimitsDto, Reading};
use serde_json::json;
use std::collections::BTreeSet;

fn archive(harness: &Harness, provider: AgentId, ids: &[String]) {
    crate::limits::set_archived_at(harness.path(), provider, ids, true).unwrap();
}

fn archived(harness: &Harness, provider: AgentId) -> BTreeSet<String> {
    crate::limits::archived(harness.path(), provider)
}

fn legacy_id(identity: &Identity) -> String {
    if identity.provider == AgentId::Codex {
        identity.workspace_id.clone()
    } else {
        identity.user_id.clone()
    }
}

fn archived_beside_another(harness: &Harness, identity: &Identity, email: &str) {
    let history = ProviderLimitsDto {
        current_account: false,
        ..ProviderLimitsDto::for_test(identity.provider, &legacy_id(identity))
            .labelled(email)
            .with_reading(Reading {
                windows: vec![LimitWindowDto {
                    id: "weekly".into(),
                    label: "Weekly".into(),
                    kind: LimitWindowKind::Weekly,
                    used_percent: 42.0,
                    window_seconds: Some(604_800),
                    resets_at: None,
                    observed_at: "2026-09-10T00:00:00Z".into(),
                }],
                ..Reading::default()
            })
    };
    assert!(crate::limits::remember(harness.path(), history)
        .saved
        .is_ok());
    archive(
        harness,
        identity.provider,
        &[
            identity.observation_key(),
            legacy_id(identity),
            "profile:other".into(),
        ],
    );
}

fn codex_with_email(user: &str, generation: &str) -> Login {
    use base64::Engine;
    let payload = json!({
        "email": format!("{user}@example.com"),
        "https://api.openai.com/auth": {"chatgpt_user_id": user, "chatgpt_account_id": "team"}
    });
    let mut login = codex(user, generation);
    login.auth["tokens"]["id_token"] = json!(format!(
        "header.{}.signature",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string())
    ));
    login
}

#[test]
fn saving_the_current_login_unarchives_its_account() {
    let harness = Harness::new();
    let a = identity(AgentId::Claude, "a", "team");
    archived_beside_another(&harness, &a, "a@example.com");
    harness.signed_in(Some(claude("a", "a1")));

    harness.accounts().save_current(AgentId::Claude).unwrap();

    assert_eq!(
        archived(&harness, AgentId::Claude),
        BTreeSet::from(["profile:other".to_string()])
    );
}

#[test]
fn a_finished_sign_in_unarchives_its_account() {
    for (provider, login) in [
        (AgentId::Claude, claude("b", "b1")),
        (AgentId::Codex, codex_with_email("b", "b1")),
    ] {
        let harness = Harness::new();
        let b = identity(provider, "b", "team");
        archived_beside_another(&harness, &b, "b@example.com");
        *harness.native.signed_in.borrow_mut() = Some(login);

        harness.native.sign_in_exit.set(1);
        let operation = uuid::Uuid::new_v4().to_string();
        assert!(harness.accounts().add(provider, operation, None).is_err());
        assert_eq!(
            archived(&harness, provider).len(),
            3,
            "{provider:?}: an unfinished sign-in unarchives nothing"
        );

        harness.native.sign_in_exit.set(0);
        let operation = uuid::Uuid::new_v4().to_string();
        harness.accounts().add(provider, operation, None).unwrap();
        assert_eq!(
            archived(&harness, provider),
            BTreeSet::from(["profile:other".to_string()]),
            "{provider:?}"
        );

        archive(&harness, provider, &[b.observation_key(), legacy_id(&b)]);
        let again = harness.vault().profiles[0].id.clone();
        let operation = uuid::Uuid::new_v4().to_string();
        harness
            .accounts()
            .add(provider, operation, Some(again))
            .unwrap();
        assert_eq!(
            archived(&harness, provider),
            BTreeSet::from(["profile:other".to_string()]),
            "{provider:?}: sign in again"
        );
    }
}

#[test]
fn automatic_remembering_never_unarchives() {
    let harness = Harness::new();
    std::fs::write(
        harness.path().join(".on-n-off/accounts/remembering.json"),
        "{\"enabled\":true}",
    )
    .unwrap();
    let a = identity(AgentId::Claude, "a", "team");
    archived_beside_another(&harness, &a, "a@example.com");
    harness.signed_in(Some(claude("a", "a1")));

    assert_eq!(harness.accounts().remember(AgentId::Claude), Ok(true));

    assert!(archived(&harness, AgentId::Claude).contains(&a.observation_key()));
}
