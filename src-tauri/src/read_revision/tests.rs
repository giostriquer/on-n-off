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

#[test]
fn every_source_has_its_own_wire_name() {
    // These strings are the contract with `SharedReadSource` in `ui/src/lib/types.ts`.
    let names = [
        Source::LimitsClaude.name(),
        Source::LimitsCodex.name(),
        Source::GithubPrs.name(),
    ];
    assert_eq!(names, ["limits:claude", "limits:codex", "github:prs"]);
    assert_eq!(
        names.iter().collect::<std::collections::HashSet<_>>().len(),
        names.len(),
        "two sources sharing a name would refetch each other's query"
    );
}
