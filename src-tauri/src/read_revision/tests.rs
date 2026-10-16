use super::*;

#[test]
fn a_replacement_moves_the_revision_and_reports_the_one_it_set() {
    let revision = Revision::new();
    assert_eq!(revision.current(), 0, "nothing has produced a value yet");
    assert_eq!(
        revision.bump(),
        1,
        "the caller stamps its snapshot with this, so it must be the value now current"
    );
    assert_eq!(revision.current(), 1);
    assert_eq!(revision.bump(), 2);
}

#[test]
fn only_a_replacement_is_worth_announcing() {
    assert!(Reading::Replaced(3).replaced());
    assert!(
        !Reading::Unchanged(3).replaced(),
        "announcing a read that changed nothing would make every answer to an announcement \
         announce again"
    );
    assert_eq!(Reading::Replaced(3).revision(), 3);
    assert_eq!(Reading::Unchanged(3).revision(), 3);
}

fn after(source: Source) -> Option<Source> {
    match source {
        Source::Accounts => Some(Source::LimitsClaude),
        Source::LimitsClaude => Some(Source::LimitsCodex),
        Source::LimitsCodex => Some(Source::LimitsMuse),
        Source::LimitsMuse => Some(Source::GithubPrs),
        Source::GithubPrs => Some(Source::ResetSpends),
        Source::ResetSpends => None,
    }
}

fn every_source() -> Vec<Source> {
    std::iter::successors(Some(Source::Accounts), |&source| after(source)).collect()
}

#[test]
fn the_announcements_the_ui_listens_for_are_the_ones_rust_sends() {
    let announcements: serde_json::Map<String, serde_json::Value> = every_source()
        .into_iter()
        .map(|source| {
            let payload = Announcement {
                source: source.name(),
            };
            (
                source.name().to_string(),
                serde_json::to_value(payload).unwrap(),
            )
        })
        .collect();
    for (position, source) in every_source().into_iter().enumerate() {
        assert_eq!(
            source as usize, position,
            "{source:?} is listed out of the order Source declares it in"
        );
    }
    assert_eq!(
        announcements.len(),
        every_source().len(),
        "two sources sharing a name would refetch each other's queries"
    );
    let written = serde_json::json!({ "event": EVENT, "announcements": announcements });
    assert_eq!(
        written,
        serde_json::from_str::<serde_json::Value>(include_str!("fixtures/shared-reads.json"))
            .unwrap(),
        "src-tauri/src/read_revision/fixtures/shared-reads.json must read:\n{}",
        serde_json::to_string_pretty(&written).unwrap()
    );
}
