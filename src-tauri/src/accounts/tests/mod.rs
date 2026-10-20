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

#[test]
fn each_provider_keeps_the_activity_lock_file_other_app_versions_use() {
    assert_eq!(
        PROVIDERS,
        [AgentId::Claude, AgentId::Codex, AgentId::Muse],
        "activity-0.lock is Claude's and activity-1.lock Codex's in every running app version: \
         a tab reorder must not move them, and a provider gaining saved accounts takes the next \
         number"
    );
}

mod archiving;
mod categories;
mod fixture;
mod homes;
mod listing;
mod login_state;
mod profiles;
mod remembering;
mod saved_claims;
mod saved_usage;
mod sign_in_end;
mod signing_in;
mod switching;
