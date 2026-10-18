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

const PAGE: &str = "https://auth.example/device?code=ABCD-1234";

#[test]
fn a_sign_in_opens_the_page_its_provider_prints_once() {
    let harness = Harness::new();
    *harness.native.signed_in.borrow_mut() = Some(claude("b", "b1"));
    harness.native.sign_in_page.set(Some(PAGE));

    harness
        .accounts()
        .add(AgentId::Claude, operation(), None)
        .unwrap();

    assert_eq!(*harness.browser.opened.borrow(), [PAGE]);
    assert_eq!(harness.vault().profiles.len(), 1);
}

#[test]
fn a_sign_in_that_prints_no_page_opens_nothing() {
    let harness = Harness::new();
    *harness.native.signed_in.borrow_mut() = Some(claude("b", "b1"));

    harness
        .accounts()
        .add(AgentId::Claude, operation(), None)
        .unwrap();

    assert!(harness.browser.opened.borrow().is_empty());
}

#[test]
fn a_sign_in_whose_page_cannot_open_ends_instead_of_waiting() {
    let harness = Harness::new();
    *harness.native.signed_in.borrow_mut() = Some(claude("b", "b1"));
    harness.native.sign_in_page.set(Some(PAGE));
    harness.native.sign_in_waits.set(true);
    harness.browser.refuses.set(true);
    let started = std::time::Instant::now();

    let error = harness
        .accounts()
        .add(AgentId::Claude, operation(), None)
        .unwrap_err();

    assert!(
        error.starts_with("Could not open the sign-in page in a browser."),
        "{error}"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
    assert_eq!(*harness.browser.opened.borrow(), [PAGE]);
    assert_eq!(harness.sealed(), None, "no vault was written");
    assert_eq!(harness.native.cleaned.get(), 1);
    assert!(left(&harness).is_empty());
}

#[test]
fn canceling_a_waiting_sign_in_ends_it_at_once() {
    let harness = Harness::new();
    *harness.native.signed_in.borrow_mut() = Some(claude("b", "b1"));
    harness.native.sign_in_waits.set(true);
    let id = operation();
    let logins = harness.path().join(".on-n-off/accounts/logins");
    let canceler = {
        let id = id.clone();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            while started.elapsed() < std::time::Duration::from_secs(20) {
                let running = std::fs::read_dir(&logins)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|entry| entry.path().join("bin").exists());
                if running {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            crate::accounts::cancel(&id);
        })
    };
    let started = std::time::Instant::now();

    let error = harness
        .accounts()
        .add(AgentId::Claude, id, None)
        .unwrap_err();
    canceler.join().unwrap();

    assert!(
        error.starts_with("Sign-in was canceled, timed out, or did not complete."),
        "{error}"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
    assert_eq!(harness.sealed(), None, "no vault was written");
    assert_eq!(harness.native.cleaned.get(), 1);
    assert!(left(&harness).is_empty());
}

#[cfg(unix)]
#[test]
fn the_sign_in_is_given_no_input() {
    let harness = Harness::new();
    *harness.native.signed_in.borrow_mut() = Some(claude("b", "b1"));
    harness.native.sign_in_wants_no_input.set(true);

    let added = harness.accounts().add(AgentId::Claude, operation(), None);

    assert!(added.is_ok(), "{added:?}");
}

#[test]
fn claude_and_codex_open_their_own_sign_in_pages() {
    for (provider, printed) in [
        (
            AgentId::Claude,
            "https://claude.ai/oauth/authorize?code=true",
        ),
        (
            AgentId::Codex,
            "https://auth.openai.com/oauth/authorize?client_id=x",
        ),
    ] {
        let dir = crate::paths::scratch_dir("sign-in-own-page");
        let isolated = crate::accounts::adapter(provider)
            .unwrap()
            .isolated(&dir)
            .unwrap();
        assert_eq!(isolated.sign_in_page(printed), None, "{provider:?}");
    }
}
