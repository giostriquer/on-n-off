use std::collections::HashSet;

use super::*;
use crate::dto::GithubPrListDto;

type Shown = (Option<MergeKind>, Option<bool>);

const CONFLICTS: Shown = (Some(MergeKind::Conflicts), Some(true));
const CARRIED: Shown = (Some(MergeKind::Conflicts), None);
const READY: Shown = (Some(MergeKind::Ready), Some(false));
const NOTHING: Shown = (None, None);

#[derive(Clone, Copy)]
enum Reply {
    Conflicting,
    Clean,
    Unknown,
}

fn row(reply: Reply) -> GithubPrDto {
    let (mergeable, merge_state) = match reply {
        Reply::Conflicting => (Mergeability::Conflicting, MergeState::Dirty),
        Reply::Clean => (Mergeability::Mergeable, MergeState::Clean),
        Reply::Unknown => (Mergeability::Unknown, MergeState::Unknown),
    };
    let mut pr = with(|pr| {
        pr.head_ref_oid = Some("head1".into());
        pr.mergeable = mergeable;
        pr.merge_state = merge_state;
    });
    pr.merge_kind = classify(&pr);
    pr
}

fn contributed(reply: Reply) -> GithubPrDto {
    GithubPrDto {
        contributed: true,
        ..row(reply)
    }
}

fn moved(mut pr: GithubPrDto, head: Option<&str>, base: &str) -> GithubPrDto {
    pr.head_ref_oid = head.map(Into::into);
    pr.base_ref = base.into();
    pr
}

fn retained() -> GithubPrDto {
    GithubPrDto {
        merge_kind: Some(MergeKind::Conflicts),
        ..row(Reply::Unknown)
    }
}

fn list(items: Vec<GithubPrDto>) -> GithubPrListDto {
    GithubPrListDto {
        total: items.len() as u64,
        items,
    }
}

fn saved(mine: Vec<GithubPrDto>, known_answers: Option<KnownAnswers>) -> Option<Snapshot> {
    Some(Snapshot {
        data: GithubPrsData {
            viewer: Some("octocat".into()),
            mine: list(mine),
            ..GithubPrsData::default()
        },
        known_answers,
    })
}

fn recorded(viewer: &str, open: Vec<GithubPrDto>) -> Option<(String, Vec<GithubPrDto>)> {
    Some((viewer.to_string(), open))
}

#[derive(Clone)]
struct Read {
    viewer: Option<&'static str>,
    restart: bool,
    mine: Vec<GithubPrDto>,
    review: Vec<GithubPrDto>,
    unlisted: Vec<GithubPrDto>,
    merged: Vec<GithubPrDto>,
    earlier: HashSet<String>,
    shows: Vec<Shown>,
}

fn read() -> Read {
    Read {
        viewer: Some("octocat"),
        restart: false,
        mine: Vec::new(),
        review: Vec::new(),
        unlisted: Vec::new(),
        merged: Vec::new(),
        earlier: HashSet::new(),
        shows: Vec::new(),
    }
}

impl Read {
    fn mine(mut self, pr: GithubPrDto) -> Self {
        self.mine.push(pr);
        self
    }

    fn review(mut self, pr: GithubPrDto) -> Self {
        self.review.push(pr);
        self
    }

    fn unlisted(mut self, pr: GithubPrDto) -> Self {
        self.unlisted.push(pr);
        self
    }

    fn merged(mut self, pr: GithubPrDto) -> Self {
        self.merged.push(pr);
        self
    }

    fn contribution_read_earlier(mut self) -> Self {
        self.earlier.insert("PR_1".into());
        self
    }

    fn by(mut self, viewer: Option<&'static str>) -> Self {
        self.viewer = viewer;
        self
    }

    fn after_restart(mut self) -> Self {
        self.restart = true;
        self
    }

    fn shows(mut self, shown: &[Shown]) -> Self {
        self.shows = shown.to_vec();
        self
    }
}

fn settle_each(case: &str, start: &KnownAnswers, reads: &[Read]) {
    let mut known = start.clone();
    for (index, read) in reads.iter().cloned().enumerate() {
        if read.restart {
            let saved = serde_json::to_string(&known).unwrap();
            known = serde_json::from_str(&saved).unwrap();
        }
        let mut data = GithubPrsData {
            viewer: read.viewer.map(Into::into),
            mine: list(read.mine),
            review_requested: list(read.review),
            merged: list(read.merged.clone()),
            ..GithubPrsData::default()
        };
        known.settle(&mut data, read.unlisted, &read.earlier);
        let shown: Vec<Shown> = data
            .open_prs()
            .map(|pr| (pr.merge_kind, facts(pr).conflicts))
            .collect();
        assert_eq!(shown, read.shows, "{case}, read {index}");
        assert_eq!(data.merged.items, read.merged, "{case}, read {index}");
    }
}

#[test]
fn a_known_conflict_is_carried_across_reads_for_the_same_revision_and_viewer_only() {
    use Reply::{Clean, Conflicting, Unknown};
    let nothing_known = KnownAnswers::default;
    let conflict = || read().mine(row(Conflicting)).shows(&[CONFLICTS]);
    let unknown = || read().mine(row(Unknown));
    let saved_memory = KnownAnswers {
        viewer: Some("octocat".into()),
        answers: vec![Known {
            revision: Revision::of(&row(Conflicting)).unwrap(),
            answer: Answer::of(&row(Conflicting)),
        }],
    };
    let cases: Vec<(&str, KnownAnswers, Vec<Read>)> = vec![
        ("a fresh conflict", nothing_known(), vec![conflict()]),
        (
            "a fresh clean answer",
            nothing_known(),
            vec![read().mine(row(Clean)).shows(&[READY])],
        ),
        (
            "unknown with nothing known",
            nothing_known(),
            vec![unknown().shows(&[NOTHING])],
        ),
        (
            "unknown after a conflict",
            nothing_known(),
            vec![
                conflict(),
                unknown().shows(&[CARRIED]),
                unknown().shows(&[CARRIED]),
            ],
        ),
        (
            "a clean answer ends it",
            nothing_known(),
            vec![
                conflict(),
                read().mine(row(Clean)).shows(&[READY]),
                unknown().shows(&[NOTHING]),
            ],
        ),
        (
            "a new head commit",
            nothing_known(),
            vec![
                conflict(),
                read()
                    .mine(moved(row(Unknown), Some("head2"), "main"))
                    .shows(&[NOTHING]),
            ],
        ),
        (
            "a new target branch",
            nothing_known(),
            vec![
                conflict(),
                read()
                    .mine(moved(row(Unknown), Some("head1"), "dev"))
                    .shows(&[NOTHING]),
            ],
        ),
        (
            "no head commit",
            nothing_known(),
            vec![
                read()
                    .mine(moved(row(Conflicting), None, "main"))
                    .shows(&[CONFLICTS]),
                read()
                    .mine(moved(row(Unknown), None, "main"))
                    .shows(&[NOTHING]),
            ],
        ),
        (
            "an empty head commit",
            nothing_known(),
            vec![
                read()
                    .mine(moved(row(Conflicting), Some(""), "main"))
                    .shows(&[CONFLICTS]),
                read()
                    .mine(moved(row(Unknown), Some(""), "main"))
                    .shows(&[NOTHING]),
            ],
        ),
        (
            "another pull request",
            nothing_known(),
            vec![
                conflict(),
                read()
                    .mine(GithubPrDto {
                        id: "PR_2".into(),
                        ..row(Unknown)
                    })
                    .shows(&[NOTHING]),
            ],
        ),
        (
            "another viewer",
            nothing_known(),
            vec![
                conflict(),
                unknown().by(Some("hubot")).shows(&[NOTHING]),
                unknown().shows(&[NOTHING]),
            ],
        ),
        (
            "the same viewer in another case",
            nothing_known(),
            vec![conflict(), unknown().by(Some("OctoCat")).shows(&[CARRIED])],
        ),
        (
            "no viewer",
            nothing_known(),
            vec![conflict().by(None), unknown().by(None).shows(&[NOTHING])],
        ),
        (
            "a restart",
            nothing_known(),
            vec![
                conflict(),
                unknown().after_restart().shows(&[CARRIED]),
                unknown().after_restart().shows(&[CARRIED]),
            ],
        ),
        (
            "another list answers conflicting",
            nothing_known(),
            vec![read()
                .mine(row(Unknown))
                .review(row(Conflicting))
                .shows(&[CONFLICTS, CONFLICTS])],
        ),
        (
            "another list answers clean over a known conflict",
            nothing_known(),
            vec![
                conflict(),
                read()
                    .mine(row(Unknown))
                    .review(row(Clean))
                    .shows(&[READY, READY]),
                unknown().shows(&[NOTHING]),
            ],
        ),
        (
            "another list answers for another revision",
            nothing_known(),
            vec![
                read()
                    .mine(row(Unknown))
                    .review(moved(row(Conflicting), Some("head2"), "main"))
                    .review(moved(row(Conflicting), Some("head1"), "dev"))
                    .review(GithubPrDto {
                        id: "PR_2".into(),
                        ..row(Conflicting)
                    })
                    .shows(&[NOTHING, CONFLICTS, CONFLICTS, CONFLICTS]),
                unknown().shows(&[NOTHING]),
            ],
        ),
        (
            "two lists answering differently, conflict first",
            nothing_known(),
            vec![
                read()
                    .mine(row(Conflicting))
                    .review(row(Clean))
                    .shows(&[CONFLICTS, READY]),
                read()
                    .mine(row(Unknown))
                    .review(row(Unknown))
                    .shows(&[CARRIED, CARRIED]),
            ],
        ),
        (
            "two lists answering differently, clean first",
            nothing_known(),
            vec![
                read()
                    .mine(row(Clean))
                    .review(row(Conflicting))
                    .shows(&[READY, CONFLICTS]),
                read()
                    .mine(row(Unknown))
                    .review(row(Unknown))
                    .shows(&[CARRIED, CARRIED]),
            ],
        ),
        (
            "every list unknown after a conflict",
            nothing_known(),
            vec![
                read()
                    .mine(row(Conflicting))
                    .review(row(Unknown))
                    .shows(&[CONFLICTS, CONFLICTS]),
                read()
                    .mine(row(Unknown))
                    .review(row(Unknown))
                    .shows(&[CARRIED, CARRIED]),
            ],
        ),
        (
            "a merge",
            nothing_known(),
            vec![
                conflict(),
                read().merged(row(Unknown)).shows(&[]),
                unknown().shows(&[NOTHING]),
            ],
        ),
        (
            "a pull request the read no longer lists",
            nothing_known(),
            vec![conflict(), read().shows(&[]), unknown().shows(&[NOTHING])],
        ),
        (
            "a scope expansion",
            nothing_known(),
            vec![
                read()
                    .review(row(Unknown))
                    .unlisted(contributed(Conflicting))
                    .shows(&[CONFLICTS]),
                read()
                    .mine(contributed(Unknown))
                    .review(row(Unknown))
                    .shows(&[CARRIED, CARRIED]),
                read()
                    .mine(contributed(Unknown))
                    .review(row(Unknown))
                    .after_restart()
                    .shows(&[CARRIED, CARRIED]),
            ],
        ),
        (
            "a contribution out of scope while unknown",
            nothing_known(),
            vec![
                conflict(),
                read().unlisted(contributed(Unknown)).shows(&[]),
                unknown().shows(&[CARRIED]),
            ],
        ),
        (
            "a listed clean answer clears an unlisted conflict",
            nothing_known(),
            vec![
                read().unlisted(contributed(Conflicting)).shows(&[]),
                read()
                    .review(row(Clean))
                    .unlisted(contributed(Unknown))
                    .shows(&[READY]),
                unknown().shows(&[NOTHING]),
            ],
        ),
        (
            "an earlier clean contribution under this read's conflict",
            nothing_known(),
            vec![
                read().mine(contributed(Clean)).shows(&[READY]),
                read()
                    .mine(contributed(Clean))
                    .review(row(Conflicting))
                    .contribution_read_earlier()
                    .shows(&[CONFLICTS, CONFLICTS]),
                read()
                    .mine(contributed(Unknown))
                    .review(row(Unknown))
                    .shows(&[CARRIED, CARRIED]),
            ],
        ),
        (
            "an earlier conflicting contribution under this read's clean answer",
            nothing_known(),
            vec![
                read().mine(contributed(Conflicting)).shows(&[CONFLICTS]),
                read()
                    .mine(contributed(Conflicting))
                    .review(row(Clean))
                    .contribution_read_earlier()
                    .shows(&[READY, READY]),
                read()
                    .mine(contributed(Unknown))
                    .review(row(Unknown))
                    .shows(&[NOTHING, NOTHING]),
            ],
        ),
        (
            "an earlier clean contribution with nothing newer",
            nothing_known(),
            vec![
                read().mine(contributed(Clean)).shows(&[READY]),
                read()
                    .mine(contributed(Clean))
                    .review(row(Unknown))
                    .contribution_read_earlier()
                    .shows(&[READY, READY]),
            ],
        ),
        (
            "an earlier conflicting contribution with nothing newer",
            nothing_known(),
            vec![
                read().mine(contributed(Conflicting)).shows(&[CONFLICTS]),
                read()
                    .mine(contributed(Conflicting))
                    .review(row(Unknown))
                    .contribution_read_earlier()
                    .shows(&[CONFLICTS, CONFLICTS]),
                read()
                    .mine(contributed(Unknown))
                    .review(row(Unknown))
                    .shows(&[CARRIED, CARRIED]),
            ],
        ),
        (
            "an earlier unlisted conflict is known at once",
            nothing_known(),
            vec![
                read()
                    .review(row(Unknown))
                    .review(GithubPrDto {
                        id: "PR_2".into(),
                        ..row(Clean)
                    })
                    .unlisted(contributed(Conflicting))
                    .contribution_read_earlier()
                    .shows(&[CONFLICTS, READY]),
                read().review(row(Unknown)).shows(&[CARRIED]),
            ],
        ),
        (
            "failing lookups after a clean answer, then a conflict",
            nothing_known(),
            vec![
                read()
                    .mine(contributed(Clean))
                    .review(row(Clean))
                    .shows(&[READY, READY]),
                read()
                    .mine(contributed(Clean))
                    .review(row(Conflicting))
                    .contribution_read_earlier()
                    .shows(&[CONFLICTS, CONFLICTS]),
                read()
                    .mine(contributed(Clean))
                    .review(row(Unknown))
                    .review(GithubPrDto {
                        id: "PR_2".into(),
                        ..row(Clean)
                    })
                    .contribution_read_earlier()
                    .shows(&[CONFLICTS, CONFLICTS, READY]),
                read()
                    .mine(contributed(Unknown))
                    .review(row(Unknown))
                    .shows(&[CARRIED, CARRIED]),
                read()
                    .mine(contributed(Conflicting))
                    .review(row(Conflicting))
                    .shows(&[CONFLICTS, CONFLICTS]),
            ],
        ),
        (
            "failing lookups after a conflict, then a clean answer",
            nothing_known(),
            vec![
                read()
                    .mine(contributed(Conflicting))
                    .review(row(Conflicting))
                    .shows(&[CONFLICTS, CONFLICTS]),
                read()
                    .mine(contributed(Conflicting))
                    .review(row(Clean))
                    .contribution_read_earlier()
                    .shows(&[READY, READY]),
                read()
                    .mine(contributed(Conflicting))
                    .review(row(Unknown))
                    .contribution_read_earlier()
                    .shows(&[READY, READY]),
                read()
                    .mine(contributed(Unknown))
                    .review(row(Unknown))
                    .shows(&[NOTHING, NOTHING]),
                read()
                    .mine(contributed(Clean))
                    .review(row(Clean))
                    .shows(&[READY, READY]),
            ],
        ),
        (
            "a clean answer is not kept through a recompute",
            nothing_known(),
            vec![
                read().mine(contributed(Clean)).shows(&[READY]),
                read().mine(contributed(Unknown)).shows(&[NOTHING]),
                read()
                    .mine(contributed(Unknown))
                    .contribution_read_earlier()
                    .shows(&[NOTHING]),
            ],
        ),
        (
            "a verdict a legacy file stored on an unknown row",
            nothing_known(),
            vec![read().mine(retained()).shows(&[NOTHING])],
        ),
        (
            "a conflict saved before the memory",
            KnownAnswers::recall(saved(vec![retained()], None), None),
            vec![unknown().shows(&[CARRIED])],
        ),
        (
            "a saved unknown row without a conflict",
            KnownAnswers::recall(saved(vec![row(Unknown)], None), None),
            vec![unknown().shows(&[NOTHING])],
        ),
        (
            "a saved verdict GitHub's answer contradicts",
            KnownAnswers::recall(
                saved(
                    vec![GithubPrDto {
                        merge_kind: Some(MergeKind::Conflicts),
                        ..row(Clean)
                    }],
                    None,
                ),
                None,
            ),
            vec![unknown().shows(&[NOTHING])],
        ),
        (
            "a saved conflict without a head commit",
            KnownAnswers::recall(saved(vec![moved(retained(), None, "main")], None), None),
            vec![unknown().shows(&[NOTHING])],
        ),
        (
            "a conflict saved before the memory, while the contribution is not looked up",
            KnownAnswers::recall(
                saved(vec![retained()], None),
                recorded(
                    "octocat",
                    vec![GithubPrDto {
                        contributed: true,
                        ..retained()
                    }],
                ),
            ),
            vec![read()
                .mine(GithubPrDto {
                    contributed: true,
                    ..retained()
                })
                .review(row(Unknown))
                .contribution_read_earlier()
                .shows(&[CARRIED, CARRIED])],
        ),
        (
            "a conflict the contribution record saved",
            KnownAnswers::recall(None, recorded("octocat", vec![retained()])),
            vec![unknown().shows(&[CARRIED])],
        ),
        (
            "a saved memory",
            KnownAnswers::recall(saved(Vec::new(), Some(saved_memory)), None),
            vec![unknown().shows(&[CARRIED])],
        ),
        (
            "a saved memory over the contribution record",
            KnownAnswers::recall(
                saved(Vec::new(), Some(KnownAnswers::default())),
                recorded("octocat", vec![retained()]),
            ),
            vec![unknown().shows(&[NOTHING])],
        ),
        (
            "a contribution record of another viewer",
            KnownAnswers::recall(saved(Vec::new(), None), recorded("hubot", vec![retained()])),
            vec![unknown().shows(&[NOTHING])],
        ),
    ];
    for (case, start, reads) in &cases {
        settle_each(case, start, reads);
    }
}

#[test]
fn a_read_without_a_viewer_keeps_no_answers() {
    let mut known = KnownAnswers::default();
    let mut data = GithubPrsData {
        mine: list(vec![row(Reply::Conflicting)]),
        ..GithubPrsData::default()
    };
    known.settle(&mut data, Vec::new(), &HashSet::new());
    assert_eq!(data.mine.items[0].merge_kind, Some(MergeKind::Conflicts));
    assert_eq!(known, KnownAnswers::default());
    assert_eq!(serde_json::to_string(&known).unwrap(), r#"{"answers":[]}"#);
}
