//! What a poll's card keeps of what its account remembers: from the account's file when the poll
//! answered, from the card it replaces when it failed.
use super::*;

#[test]
fn a_failed_inactive_read_preserves_its_last_numbers_and_timestamp() {
    let profile = profile();
    let mut entries = vec![];
    merge(&mut entries, &profile, Some(Err("paused".into())));
    entries[0].reading.windows.push(crate::dto::LimitWindowDto {
        id: "weekly".into(),
        label: "Weekly".into(),
        kind: crate::dto::LimitWindowKind::Weekly,
        used_percent: 73.0,
        resets_at: None,
        observed_at: "2026-09-01T00:00:00Z".into(),
        window_seconds: Some(604800),
    });
    merge(&mut entries, &profile, Some(Err("unavailable".into())));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reading.windows[0].used_percent, 73.0);
    assert_eq!(
        entries[0].reading.windows[0].observed_at,
        "2026-09-01T00:00:00Z"
    );
    assert_eq!(entries[0].status, LimitsStatus::Failed);
    assert!(!entries[0].current_account);
}
/// A poll that could not tell the banked-reset count keeps the one its account's file holds; one that
/// answered replaces it.
#[test]
fn a_saved_read_that_cannot_tell_keeps_the_banked_reset_count_and_an_answer_replaces_it() {
    use crate::dto::LimitsResetCreditsDto;
    let home = tempfile::tempdir().unwrap();
    let p = stored(home.path());
    let banked = |available_count| {
        Some(LimitsResetCreditsDto {
            available_count,
            next_expires_at: None,
        })
    };
    let answer = |reset_credits| {
        let mut dto = reading(&p);
        dto.reading.reset_credits = reset_credits;
        dto
    };

    polled(home.path(), &p, &answer(banked(1)));
    let card = polled(home.path(), &p, &answer(None));
    assert_eq!(card.reading.reset_credits, banked(1));

    let card = polled(home.path(), &p, &answer(banked(0)));
    assert_eq!(card.reading.reset_credits, banked(0));
}

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

/// Refreshes rebuild the cards from the snapshots on disk, so the count has to survive there, not
/// only in the card the first poll after switching away merges into.
#[test]
fn a_saved_poll_that_cannot_tell_keeps_the_remembered_banked_reset_count_across_refreshes() {
    use crate::dto::LimitsResetCreditsDto;
    use crate::limits::{remember, remembered};
    let home = tempfile::tempdir().unwrap();
    let p = stored(home.path());
    let banked = Some(LimitsResetCreditsDto {
        available_count: 1,
        next_expires_at: None,
    });
    let mut reading_with_count = reading(&p);
    reading_with_count.reading.reset_credits.clone_from(&banked);
    remember(home.path(), reading_with_count).saved.unwrap();

    for (refresh, observed_at) in ["2026-09-20T00:00:00Z", "2026-09-21T00:00:00Z"]
        .into_iter()
        .enumerate()
    {
        let mut entries = remembered(home.path(), AgentId::Claude);
        let result = poll_with(
            home.path(),
            &p,
            &ticket(home.path()),
            true,
            &|| Ok(open(home.path())),
            &|p| {
                let mut dto = reading(p);
                dto.reading.windows[0].observed_at = observed_at.into();
                FetchResult {
                    login: p.login.clone(),
                    result: Ok(dto),
                }
            },
        );
        merge(&mut entries, &p, result);
        assert_eq!(
            entries[0].reading.reset_credits, banked,
            "refresh {refresh}"
        );
    }
}

/// A saved read whose spending read failed or was backing off keeps the figure the card had.
#[test]
fn a_saved_read_that_could_not_tell_what_was_spent_keeps_the_cards_figure() {
    let home = tempfile::tempdir().unwrap();
    let p = stored_codex(home.path());
    let spent = Some(crate::dto::LimitsCreditsSpentDto {
        last_7_days: 18303.4,
        last_30_days: 20299.7,
        updated_at: None,
    });
    let mut first = codex_reading(&p, "business");
    first.reading.credits_spent.clone_from(&spent);
    polled(home.path(), &p, &first);

    let card = polled(home.path(), &p, &codex_reading(&p, "business"));

    assert_eq!(card.reading.credits_spent, spent);
}

/// A saved read whose term read failed or was backing off keeps the term the card had.
#[test]
fn a_saved_read_that_could_not_tell_the_term_keeps_the_cards_term() {
    let home = tempfile::tempdir().unwrap();
    let p = stored_codex(home.path());
    let term = Some(crate::dto::LimitsSubscriptionDto {
        active_until: "2026-09-28T16:22:34Z".to_string(),
        will_renew: false,
        note: Some(crate::dto::SubscriptionNote::Cancelled),
        checked_at: "2026-09-25T12:00:00Z".to_string(),
    });
    let mut first = codex_reading(&p, "pro");
    first.reading.subscription.clone_from(&term);
    polled(home.path(), &p, &first);

    let card = polled(home.path(), &p, &codex_reading(&p, "pro"));

    assert_eq!(card.reading.subscription, term);
}

/// `stored`, as a saved Codex profile.
fn stored_codex(home: &Path) -> Profile {
    let mut p = stored(home);
    p.identity.provider = AgentId::Codex;
    rewrite(home, |db| db.profiles[0] = p.clone());
    p
}

/// `answer`, polled for `p` over what `home` remembers, as the card the poll leaves in the list.
fn polled(home: &Path, p: &Profile, answer: &ProviderLimitsDto) -> ProviderLimitsDto {
    let mut entries = crate::limits::remembered(home, p.identity.provider);
    let result = poll_with(home, p, &ticket(home), true, &|| Ok(open(home)), &|p| {
        FetchResult {
            login: p.login.clone(),
            result: Ok(answer.clone()),
        }
    });
    merge(&mut entries, p, result);
    let key = p.identity.observation_key();
    entries
        .into_iter()
        .find(|card| {
            card.account
                .as_ref()
                .is_some_and(|account| account.id == key)
        })
        .expect("the profile's card")
}

/// A saved Codex read on `plan`.
fn codex_reading(profile: &Profile, plan: &str) -> ProviderLimitsDto {
    let mut dto = reading(profile);
    dto.provider = AgentId::Codex;
    dto.reading.plan = Some(plan.to_string());
    dto
}

/// An account now on a personal plan pools nothing and is never asked what it spent: the figure it
/// had on a workspace plan goes, rather than staying on its card for good.
#[test]
fn a_saved_personal_plan_read_drops_the_cards_figure() {
    let home = tempfile::tempdir().unwrap();
    let p = stored_codex(home.path());
    let mut first = codex_reading(&p, "business");
    first.reading.credits_spent = Some(crate::dto::LimitsCreditsSpentDto {
        last_7_days: 18303.4,
        last_30_days: 20299.7,
        updated_at: None,
    });
    polled(home.path(), &p, &first);

    let card = polled(home.path(), &p, &codex_reading(&p, "pro"));

    assert_eq!(card.reading.credits_spent, None);
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
        "subscriptionStatus": "active",
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
    assert_eq!(serde_json::to_value(&entries[0]).unwrap(), expected);
    assert_eq!(
        serde_json::to_value(&remembered(home.path(), AgentId::Codex)[0]).unwrap(),
        expected
    );
}
