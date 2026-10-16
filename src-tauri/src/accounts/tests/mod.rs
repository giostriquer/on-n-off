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

#[test]
fn saved_accounts_have_an_adapter_exactly_where_the_catalogue_offers_them() {
    for id in AgentId::ALL {
        assert_eq!(adapter(id).is_ok(), id.facts().saved_accounts, "{id:?}");
    }
}

mod archiving;
mod fixture;
mod homes;
mod listing;
mod profiles;
mod remembering;
mod saved_claims;
mod saved_usage;
mod signing_in;
mod switching;
