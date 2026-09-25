//! A signed-in business card's spending, through the remembered-accounts pipeline.

use crate::dto::{LimitWindowKind, LimitsCreditsSpentDto};
use crate::limits::json::window;
use crate::limits::*;
use crate::paths::scratch_dir;

fn spent(last_7_days: f64) -> Option<LimitsCreditsSpentDto> {
    Some(LimitsCreditsSpentDto {
        last_7_days,
        last_30_days: last_7_days + 2000.0,
        updated_at: Some("2026-09-24T19:00:00Z".to_string()),
    })
}

/// A signed-in business member's card as the Codex read hands it over: its weekly window, its own
/// balance of 0, and what it spent.
fn business_card(
    id: &str,
    status: LimitsStatus,
    credits_spent: Option<LimitsCreditsSpentDto>,
) -> ProviderLimitsDto {
    let ok = status == LimitsStatus::Ok;
    let mut dto = finish(
        AgentId::Codex,
        status,
        (!ok).then(|| "Refresh failed".to_string()),
        Parsed {
            account: Some(LimitsAccountDto {
                legacy_id: None,
                id: id.to_string(),
                label: Some(format!("{id}@example.com")),
            }),
            plan: ok.then(|| "self_serve_business_prolite".to_string()),
            windows: if ok {
                vec![window(
                    "primary",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    12.0,
                    None,
                )]
            } else {
                Vec::new()
            },
            credits: ok.then(|| LimitsCreditsDto {
                balance: "0".to_string(),
                unlimited: false,
            }),
            credits_spent,
            ..Parsed::default()
        },
    );
    for window in &mut dto.windows {
        window.observed_at = "2026-09-24T12:00:00.000Z".to_string();
    }
    dto
}

fn card<'a>(listed: &'a [ProviderLimitsDto], id: &str) -> &'a ProviderLimitsDto {
    listed
        .iter()
        .find(|dto| dto.account.as_ref().is_some_and(|account| account.id == id))
        .unwrap()
}

/// Switching accounts leaves the business card remembered with what it spent, so it never falls
/// back to its own balance of 0.
#[test]
fn a_business_card_keeps_what_it_spent_after_an_account_switch() {
    let home = scratch_dir("limits-spent-switch");
    let store = SnapshotStore::for_home(&home);
    aggregate_accounts(
        &store,
        business_card("acct-a", LimitsStatus::Ok, spent(18303.4)),
    );

    let listed = aggregate_accounts(&store, business_card("acct-b", LimitsStatus::Ok, None));

    let remembered = card(&listed, "acct-a");
    assert!(!remembered.current_account);
    assert_eq!(remembered.credits_spent, spent(18303.4));
    let _ = std::fs::remove_dir_all(&home);
}

/// A failed app-server read keeps the card's last figure beside its remembered windows.
#[test]
fn a_failed_signed_in_read_keeps_what_the_card_spent() {
    let home = scratch_dir("limits-spent-failed");
    let store = SnapshotStore::for_home(&home);
    aggregate_accounts(
        &store,
        business_card("acct-a", LimitsStatus::Ok, spent(18303.4)),
    );

    let listed = aggregate_accounts(&store, business_card("acct-a", LimitsStatus::Failed, None));

    let current = card(&listed, "acct-a");
    assert_eq!(current.status, LimitsStatus::Failed);
    assert_eq!(current.credits_spent, spent(18303.4));
}

/// A read whose spending read failed or was backing off could not tell what was spent: like a read
/// that could not tell the banked-reset count, it keeps the figure it had, on the card and on disk.
#[test]
fn a_read_that_could_not_tell_what_was_spent_keeps_the_remembered_figure() {
    let home = scratch_dir("limits-spent-kept");
    let store = SnapshotStore::for_home(&home);
    aggregate_accounts(
        &store,
        business_card("acct-a", LimitsStatus::Ok, spent(18303.4)),
    );

    let listed = aggregate_accounts(&store, business_card("acct-a", LimitsStatus::Ok, None));

    assert_eq!(card(&listed, "acct-a").credits_spent, spent(18303.4));
    assert_eq!(store.load(AgentId::Codex)[0].credits_spent, spent(18303.4));

    // A read that answered replaces it.
    let listed = aggregate_accounts(
        &store,
        business_card("acct-a", LimitsStatus::Ok, spent(5.0)),
    );
    assert_eq!(card(&listed, "acct-a").credits_spent, spent(5.0));
}

/// An account that moved to a personal plan pools nothing and is never asked again, so its next read
/// drops the figure it had on a workspace plan, on the card and on disk.
#[test]
fn a_personal_plan_read_drops_the_remembered_figure() {
    let home = scratch_dir("limits-spent-personal");
    let store = SnapshotStore::for_home(&home);
    aggregate_accounts(
        &store,
        business_card("acct-a", LimitsStatus::Ok, spent(18303.4)),
    );
    let mut personal = business_card("acct-a", LimitsStatus::Ok, None);
    personal.plan = Some("pro".to_string());

    let listed = aggregate_accounts(&store, personal);

    assert_eq!(card(&listed, "acct-a").credits_spent, None);
    assert_eq!(store.load(AgentId::Codex)[0].credits_spent, None);
}

/// "team" is a Claude plan too; only a Codex workspace card is ever asked what it spent.
#[test]
fn only_a_codex_workspace_card_is_asked_what_it_spent() {
    let mut card = business_card("acct-a", LimitsStatus::Ok, None);
    assert!(credits_spent::asks_what_was_spent(&card));

    card.plan = Some("pro".to_string());
    assert!(!credits_spent::asks_what_was_spent(&card));

    card.provider = AgentId::Claude;
    card.plan = Some("team".to_string());
    assert!(!credits_spent::asks_what_was_spent(&card));
}
