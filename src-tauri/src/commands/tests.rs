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

#[test]
fn a_category_is_offered_only_to_providers_with_limits_accounts() {
    for id in AgentId::ALL
        .into_iter()
        .filter(|id| !id.facts().limits_reader)
    {
        let refused =
            tauri::async_runtime::block_on(set_account_category(id, "acct".into(), "Work".into()))
                .unwrap_err();
        assert_eq!(
            refused.message, "This provider has no Limits accounts to name.",
            "{id:?}"
        );
    }
}
