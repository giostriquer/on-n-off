//! A Codex card's subscription term, through the remembered-accounts pipeline.

use crate::dto::{LimitWindowKind, LimitsSubscriptionDto, SubscriptionNote};
use crate::limits::json::window;
use crate::limits::*;
use crate::paths::scratch_dir;

fn term(will_renew: bool) -> Option<LimitsSubscriptionDto> {
    Some(LimitsSubscriptionDto {
        active_until: "2026-09-28T16:22:34Z".to_string(),
        will_renew,
        note: (!will_renew).then_some(SubscriptionNote::Cancelled),
        checked_at: "2026-09-25T12:00:00Z".to_string(),
    })
}

/// A Codex card as the read hands it over: its weekly window and, when the term read answered, its term.
fn codex_card(
    id: &str,
    status: LimitsStatus,
    subscription: Option<LimitsSubscriptionDto>,
) -> ProviderLimitsDto {
    let ok = status == LimitsStatus::Ok;
    let mut dto = finish(
        AgentId::Codex,
        status,
        (!ok).then(|| "Refresh failed".to_string()),
        Parsed {
            windows: vec![window(
                "primary",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                42.0,
                Some("2026-09-28T23:34:33+00:00".to_string()),
            )],
            subscription,
            ..Parsed::for_card(Some(id), Some("pro"))
        },
    );
    for window in &mut dto.windows {
        window.observed_at = "2026-09-25T12:00:00.000Z".to_string();
    }
    dto
}

fn card<'a>(listed: &'a [ProviderLimitsDto], id: &str) -> &'a ProviderLimitsDto {
    listed
        .iter()
        .find(|dto| dto.account.as_ref().is_some_and(|account| account.id == id))
        .unwrap()
}

/// Switching accounts leaves the card remembered with its term.
#[test]
fn a_card_keeps_its_term_after_an_account_switch() {
    let home = scratch_dir("limits-term-switch");
    let store = SnapshotStore::for_home(&home);
    aggregate_accounts(
        &store,
        codex_card("acct-a", LimitsStatus::Ok, term(false)),
        None,
    );

    let listed = aggregate_accounts(&store, codex_card("acct-b", LimitsStatus::Ok, None), None);

    let remembered = card(&listed, "acct-a");
    assert!(!remembered.current_account);
    assert_eq!(remembered.subscription, term(false));
    let _ = std::fs::remove_dir_all(&home);
}

/// A read whose term read failed or was backing off keeps the term it had, on the card and on
/// disk; a read that answered replaces it.
#[test]
fn a_read_that_could_not_tell_the_term_keeps_the_remembered_one() {
    let home = scratch_dir("limits-term-kept");
    let store = SnapshotStore::for_home(&home);
    aggregate_accounts(
        &store,
        codex_card("acct-a", LimitsStatus::Ok, term(false)),
        None,
    );

    let listed = aggregate_accounts(&store, codex_card("acct-a", LimitsStatus::Ok, None), None);
    assert_eq!(card(&listed, "acct-a").subscription, term(false));
    assert_eq!(store.load(AgentId::Codex)[0].subscription, term(false));

    let listed = aggregate_accounts(
        &store,
        codex_card("acct-a", LimitsStatus::Ok, term(true)),
        None,
    );
    assert_eq!(card(&listed, "acct-a").subscription, term(true));
    let _ = std::fs::remove_dir_all(&home);
}
