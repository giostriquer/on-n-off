use super::cases::fixture;
use super::*;
use crate::dto::{AgentId, LimitsStatus};

#[test]
fn every_card_stands_as_its_row_says_in_the_shape_the_ui_reads() {
    for case in fixture().cases {
        assert_eq!(
            serde_json::to_value(ReadStanding::of(&case.card())).unwrap(),
            case.standing,
            "{}",
            case.name
        );
    }
}

#[test]
fn a_finished_read_decides_every_cards_standing() {
    let cases = fixture().cases;
    let mut cards: Vec<ProviderLimitsDto> = cases.iter().map(|case| case.card()).collect();
    stand(&mut cards);
    for (card, case) in cards.iter().zip(&cases) {
        assert_eq!(card.standing, case.expected(), "{}", case.name);
    }
}

#[test]
fn a_card_carries_a_standing_other_than_live_and_one_written_without_it_reads_live() {
    let mut card = ProviderLimitsDto::for_test(AgentId::Codex, "acct");
    card.status = LimitsStatus::SignedOut;
    card.message = Some("Sign in with `codex` to see subscription limits.".into());
    card.standing = ReadStanding::of(&card);
    let wire = serde_json::to_value(&card).unwrap();
    assert_eq!(
        wire["standing"],
        serde_json::json!({
            "kind": "unavailable",
            "message": { "text": "Sign in with `codex` to see subscription limits.", "tone": "muted" }
        })
    );
    let mut older = wire;
    older.as_object_mut().unwrap().remove("standing");
    let live = serde_json::from_value::<ProviderLimitsDto>(older).unwrap();
    assert_eq!(live.standing, ReadStanding::Live);
    assert!(serde_json::to_value(&live)
        .unwrap()
        .get("standing")
        .is_none());
}
