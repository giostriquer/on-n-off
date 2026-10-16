use super::*;

#[test]
fn each_provider_reaches_its_own_adapter() {
    let state = AppState::production();
    for id in AgentId::ALL {
        assert_eq!(state.adapter(id).info().id, id);
    }
}

#[test]
fn an_adapter_offers_saved_accounts_exactly_when_the_catalogue_says_so() {
    let state = AppState::production();
    for id in AgentId::ALL {
        assert_eq!(
            state.adapter(id).supports_accounts(),
            id.facts().saved_accounts,
            "{id:?}"
        );
    }
}
