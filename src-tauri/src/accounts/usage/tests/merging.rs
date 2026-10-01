//! What a poll's card keeps of what its account remembers: from the account's file when the poll
//! answered, from the card it replaces when it failed.
use super::*;

#[test]
fn inactive_results_never_replace_the_active_account() {
    let profile = profile();
    let mut entries = vec![];
    merge(&mut entries, &profile, Some(Err("first".into())));
    entries[0].current_account = true;
    let before = entries.clone();
    merge(&mut entries, &profile, Some(Err("late saved read".into())));
    assert_eq!(entries, before);
}

/// `profile` as a saved Codex account, and its card with every figure known, as the card list holds
/// it before a poll: remembered, not the signed-in account.
fn remembered_codex_card(profile: &mut Profile) -> ProviderLimitsDto {
    profile.identity.provider = AgentId::Codex;
    serde_json::from_value(json!({
        "provider": "codex",
        "status": "ok",
        "account": {"id": profile.identity.observation_key(), "label": "a@example.com"},
        "currentAccount": false,
        "plan": "business",
        "windows": [
            {"id": "primary", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 40.0,
             "observedAt": "2026-09-19T00:00:00Z"}
        ],
        "credits": {"balance": "0", "unlimited": false},
        "workspaceCredits": {"limit": "25000", "used": "8000", "usedPercent": 32.0, "reached": false},
        "creditsSpent": {"last7Days": 18303.4, "last30Days": 20299.7},
        "subscription": {"activeUntil": "2100-09-28T16:22:34Z", "willRenew": false,
                         "checkedAt": "2026-09-19T00:00:00Z"},
        "resetCredits": {"availableCount": 1}
    }))
    .unwrap()
}

/// A failed poll shows the card's whole remembered reading under the failure, its term included.
#[test]
fn a_failed_saved_read_keeps_the_cards_whole_reading() {
    let mut profile = profile();
    let remembered = remembered_codex_card(&mut profile);
    let mut entries = vec![remembered.clone()];

    merge(&mut entries, &profile, Some(Err("paused".into())));

    let mut expected = serde_json::to_value(&remembered).unwrap();
    expected["status"] = json!("failed");
    expected["message"] = json!("paused");
    expected["savedProfile"] = json!(true);
    assert_eq!(entries.len(), 1);
    assert_eq!(serde_json::to_value(&entries[0]).unwrap(), expected);
}

/// A poll that answered with its plan and one window, over a snapshot holding every figure: its
/// card and the snapshot it leaves keep the same figures, the ones it could not tell.
#[test]
fn an_answered_poll_keeps_the_same_figures_on_its_card_and_on_disk() {
    use crate::limits::{remember, remembered};
    let home = tempfile::tempdir().unwrap();
    let mut p = stored(home.path());
    remember(home.path(), remembered_codex_card(&mut p))
        .saved
        .unwrap();
    rewrite(home.path(), |db| db.profiles[0] = p.clone());
    let mut entries = remembered(home.path(), AgentId::Codex);
    let answered: ProviderLimitsDto = serde_json::from_value(json!({
        "provider": "codex",
        "status": "ok",
        "account": {"id": p.identity.observation_key(), "label": "a@example.com"},
        "currentAccount": false,
        "plan": "business",
        "windows": [
            {"id": "primary", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 50.0,
             "observedAt": "2026-09-20T00:00:00Z"}
        ]
    }))
    .unwrap();

    let result = poll_with(
        home.path(),
        &p,
        &ticket(home.path()),
        false,
        &|| Ok(open(home.path())),
        &|p| FetchResult {
            login: p.login.clone(),
            result: Ok(answered.clone()),
        },
    );
    merge(&mut entries, &p, result);

    let expected = json!({
        "provider": "codex",
        "status": "ok",
        "account": {"id": p.identity.observation_key(), "label": "a@example.com"},
        "currentAccount": false,
        "plan": "business",
        "windows": [
            {"id": "primary", "label": "Weekly · all models", "kind": "weekly",
             "usedPercent": 50.0, "observedAt": "2026-09-20T00:00:00Z"}
        ],
        "creditsSpent": {"last7Days": 18303.4, "last30Days": 20299.7},
        "subscription": {"activeUntil": "2100-09-28T16:22:34Z", "willRenew": false,
                         "checkedAt": "2026-09-19T00:00:00Z"},
        "resetCredits": {"availableCount": 1}
    });
    assert_eq!(
        serde_json::to_value(&remembered(home.path(), AgentId::Codex)[0]).unwrap(),
        expected
    );
    let mut card = expected;
    card["savedProfile"] = json!(true);
    assert_eq!(serde_json::to_value(&entries[0]).unwrap(), card);
}
