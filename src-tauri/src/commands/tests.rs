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
fn a_category_is_offered_exactly_to_providers_with_limits_accounts() {
    let refusal = "This provider has no Limits accounts to name.";
    for id in AgentId::ALL {
        let answer =
            tauri::async_runtime::block_on(set_account_category(id, "acct".into(), "Work".into()));
        let refused = answer.as_ref().err().map(|error| error.message.as_str()) == Some(refusal);
        assert_eq!(refused, !id.facts().limits.is_some(), "{id:?}");
    }
}
