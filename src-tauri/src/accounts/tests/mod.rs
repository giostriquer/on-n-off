use super::*;
use std::cell::RefCell;

#[test]
fn only_an_ordinary_switch_checks_for_safe_clients_and_recovery_requires_closed_ones() {
    for provider in [AgentId::Codex, AgentId::Claude] {
        let checks = |activation| {
            let calls = RefCell::new(Vec::new());
            check_clients(
                activation,
                provider,
                |_| {
                    calls.borrow_mut().push("activation safe");
                    Ok(())
                },
                |_| {
                    calls.borrow_mut().push("closed");
                    Ok(())
                },
            )
            .unwrap();
            calls.into_inner()
        };
        assert_eq!(checks(Activation::Ordinary), ["activation safe"]);
        assert!(checks(Activation::AlongsideClients).is_empty());
        assert_eq!(checks(Activation::Recover), ["closed"]);
    }
    let refused = check_clients(
        Activation::Recover,
        AgentId::Codex,
        |_| Ok(()),
        |_| Err("clients are running".into()),
    );
    assert_eq!(refused, Err("clients are running".into()));
}

#[test]
fn account_actions_map_to_their_activation() {
    assert_eq!(Activation::from_action("use"), Some(Activation::Ordinary));
    assert_eq!(
        Activation::from_action("useAlongsideClients"),
        Some(Activation::AlongsideClients)
    );
    assert_eq!(
        Activation::from_action("recover"),
        Some(Activation::Recover)
    );
    assert_eq!(Activation::from_action("signOut"), None);
}

mod fixture;
mod listing;
mod profiles;
mod switching;
