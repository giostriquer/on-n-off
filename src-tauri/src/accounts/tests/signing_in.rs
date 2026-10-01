use super::fixture::{claude, codex, generation, identity, Harness, Heard};
use crate::dto::AgentId;

fn operation() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn left(harness: &Harness) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(harness.path().join(".on-n-off/accounts/logins"))
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default()
}

#[test]
fn a_sign_in_saves_the_login_the_official_client_left_and_cleans_its_private_home() {
    for (provider, login) in [
        (AgentId::Claude, claude("b", "b1")),
        (AgentId::Codex, codex("b", "b1")),
    ] {
        let harness = Harness::new();
        harness.signed_in(Some(claude("a", "a1")));
        *harness.native.signed_in.borrow_mut() = Some(login);

        harness.accounts().add(provider, operation(), None).unwrap();

        let vault = harness.vault();
        assert_eq!(vault.profiles.len(), 1, "{provider:?}");
        let profile = &vault.profiles[0];
        assert_eq!(profile.identity, identity(provider, "b", "team"));
        assert_eq!(generation(profile.login.as_ref()), Some("b1".into()));
        assert!(profile.pending_activation);
        assert_eq!(
            profile.usage_renewal_owned,
            provider == AgentId::Codex,
            "{provider:?}"
        );
        assert_eq!(harness.live(), Some("a1".into()), "the CLI keeps its login");
        assert_eq!(*harness.native.resolved.borrow(), [provider]);
        assert_eq!(harness.native.cleaned.get(), 1);
        assert!(left(&harness).is_empty(), "{:?}", left(&harness));
        assert_eq!(harness.heard(), [(Heard::Changed(provider), true)]);
    }
}

#[test]
fn an_unfinished_sign_in_saves_nothing_and_still_cleans_up() {
    let harness = Harness::new();
    *harness.native.signed_in.borrow_mut() = Some(claude("b", "b1"));
    harness.native.sign_in_exit.set(1);

    let error = harness
        .accounts()
        .add(AgentId::Claude, operation(), None)
        .unwrap_err();

    assert!(
        error.starts_with("Sign-in was canceled, timed out, or did not complete."),
        "{error}"
    );
    assert_eq!(harness.sealed(), None, "no vault was written");
    assert_eq!(harness.native.cleaned.get(), 1);
    assert!(left(&harness).is_empty());
    assert!(harness.heard().is_empty());
}

fn abandoned(harness: &Harness, provider: &str) -> std::path::PathBuf {
    let dir = harness
        .path()
        .join(".on-n-off/accounts/logins/login-abandoned");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("lease"), "").unwrap();
    std::fs::write(dir.join("provider.json"), format!("\"{provider}\"")).unwrap();
    dir
}

#[test]
fn an_abandoned_sign_in_home_is_cleaned_only_once_its_clients_are_closed() {
    let harness = Harness::new();
    let dir = abandoned(&harness, "codex");
    harness.clients.running.set(true);

    harness.accounts().list(AgentId::Claude).unwrap();
    assert!(dir.exists());
    assert_eq!(harness.native.cleaned.get(), 0);
    assert_eq!(
        *harness.clients.asked.borrow(),
        [("closed", AgentId::Codex)],
        "the abandoned home's own provider's clients"
    );

    harness.clients.running.set(false);
    harness.accounts().list(AgentId::Claude).unwrap();
    assert!(!dir.exists());
    assert_eq!(harness.native.cleaned.get(), 1);
    assert_eq!(
        *harness.native.isolated.borrow(),
        [(AgentId::Codex, dir.clone()), (AgentId::Codex, dir)],
        "each listing resolved the home's own provider's isolated store"
    );
    assert!(
        !harness.native.resolved.borrow().contains(&AgentId::Codex),
        "cleaning a sign-in's home resolves no user store"
    );
}

#[test]
fn a_sign_in_home_of_a_provider_without_saved_profiles_is_left_alone() {
    let harness = Harness::new();
    let dir = abandoned(&harness, "cursor");

    harness.accounts().list(AgentId::Claude).unwrap();

    assert!(dir.exists());
    assert!(harness.clients.asked.borrow().is_empty());
    assert!(harness.native.isolated.borrow().is_empty());
}
