use super::cases::fixture;
use super::*;

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
fn the_limits_command_sends_each_card_as_it_is_with_its_standing_left_out_while_live() {
    for case in fixture().cases {
        let card = case.card();
        let mut sent = serde_json::to_value(LimitsCardDto::from(card.clone())).unwrap();
        let standing = sent.as_object_mut().unwrap().remove("standing");
        assert_eq!(
            standing,
            (case.expected() != ReadStanding::Live).then(|| case.standing.clone()),
            "{}",
            case.name
        );
        assert_eq!(sent, serde_json::to_value(&card).unwrap(), "{}", case.name);
    }
}
