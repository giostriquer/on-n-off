use super::*;

#[test]
fn a_revision_is_announced_once_however_many_readers_see_it() {
    let announced = AtomicU64::new(0);
    assert!(advanced(&announced, 1), "the read that moved the cache");
    assert!(
        !advanced(&announced, 1),
        "a cache-served read carries the same revision and must stay silent, or the \
         refetch it provokes would announce again"
    );
    assert!(advanced(&announced, 2));
    assert!(
        !advanced(&announced, 1),
        "a read that started earlier and finished late cannot walk the watermark back"
    );
    assert_eq!(announced.load(Ordering::Acquire), 2);
}

#[test]
fn every_announced_source_has_its_own_name_and_watermark() {
    let sources = [Source::LimitsClaude, Source::LimitsCodex, Source::GithubPrs];
    let names: Vec<_> = sources.iter().map(|source| source.name()).collect();
    assert_eq!(names, ["limits:claude", "limits:codex", "github:prs"]);
    for (index, source) in sources.iter().enumerate() {
        for other in &sources[index + 1..] {
            assert!(
                !std::ptr::eq(source.announced(), other.announced()),
                "{} and {} share a watermark, so one would silence the other",
                source.name(),
                other.name()
            );
        }
    }
    assert_eq!(Source::limits(AgentId::Claude), Some(Source::LimitsClaude));
    assert_eq!(
        Source::limits(AgentId::Cursor),
        None,
        "a provider with no shared cache has nothing to announce"
    );
}

#[test]
fn an_unread_source_is_revision_zero_and_every_replacement_moves_it() {
    let revision = Revision::new();
    assert_eq!(revision.current(), 0);
    assert_eq!(revision.bump(), 1, "bump reports the revision it just set");
    assert_eq!(revision.current(), 1);
    assert_eq!(revision.bump(), 2);
    assert_eq!(revision.current(), 2);
}
