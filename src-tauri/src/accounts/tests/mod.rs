use super::*;

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
