use std::collections::BTreeSet;

use super::cases::fixture;
use super::*;

#[test]
fn every_read_stands_as_its_row_says_in_the_shape_the_ui_reads() {
    for case in fixture().cases {
        assert_eq!(
            serde_json::to_value(PrReadStanding::of(&case.read())).unwrap(),
            case.standing,
            "{}",
            case.name
        );
    }
}

#[test]
fn every_status_has_a_row_with_nothing_remembered_and_one_over_the_last_read() {
    let index = |status: GithubStatus| match status {
        GithubStatus::Ok => 0,
        GithubStatus::GhMissing => 1,
        GithubStatus::GhNotLoggedIn => 2,
        GithubStatus::TokenRejected => 3,
        GithubStatus::RateLimited => 4,
        GithubStatus::Network => 5,
    };
    let rows: BTreeSet<(u8, bool)> = fixture()
        .cases
        .iter()
        .map(|case| (index(case.status), case.read().stale))
        .collect();
    let failures = (1..=5).flat_map(|status| [(status, false), (status, true)]);
    assert_eq!(
        rows,
        std::iter::once((0, false)).chain(failures).collect(),
        "an ok read, and every failure both alone and over the last read"
    );
}

#[test]
fn the_reply_sends_the_read_as_it_is_with_its_standing_left_out_while_live() {
    for case in fixture().cases {
        let read = case.read();
        let mut sent = serde_json::to_value(GithubPrsReplyDto::of(read.clone())).unwrap();
        let standing = sent.as_object_mut().unwrap().remove("standing");
        assert_eq!(
            standing,
            (case.expected() != PrReadStanding::Live).then(|| case.standing.clone()),
            "{}",
            case.name
        );
        assert_eq!(sent, serde_json::to_value(&read).unwrap(), "{}", case.name);
    }
}
