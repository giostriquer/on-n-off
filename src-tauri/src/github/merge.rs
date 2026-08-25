//! The one reading of a pull request's merge fields. The screen shows `classify`'s verdict as
//! a badge and sorts by it; the monitor compares the unknown-aware facts below between polls.
//! Nothing else interprets `mergeable`, `mergeStateStatus`, the merge queue or auto-merge.

use crate::dto::{CiState, GithubPrDto, MergeKind, MergeState, Mergeability, ReviewDecision};

/// Conflicts, then the merge queue, auto-merge, ready to merge, behind the base, blocked.
/// "Blocked" is deliberately quiet when the review decision is "review required" — that is
/// every protected pull request's default state, and the badge would otherwise sit on nearly
/// every unreviewed row — and when changes were requested or CI is not green (or absent), which
/// do explain it. Unknown, unstable and draft states say nothing.
pub(crate) fn classify(pr: &GithubPrDto) -> Option<MergeKind> {
    if has_conflicts(pr) {
        return Some(MergeKind::Conflicts);
    }
    if pr.merge_queue.is_some() {
        return Some(MergeKind::Queued);
    }
    if pr.auto_merge {
        return Some(MergeKind::AutoMerge);
    }
    match pr.merge_state {
        // GitHub reports DRAFT rather than CLEAN for a draft; the guard is defensive.
        MergeState::Clean if !pr.is_draft => Some(MergeKind::Ready),
        MergeState::Behind => Some(MergeKind::Behind),
        MergeState::Blocked if !review_explains(pr) && !ci_explains(pr) => Some(MergeKind::Blocked),
        _ => None,
    }
}

/// Whether the head merges cleanly, or `None` while GitHub has not computed it. Both fields are
/// consulted because they diverge on drafts: a draft with conflicts reports `mergeStateStatus:
/// DRAFT` while `mergeable` still says `CONFLICTING`.
pub(crate) fn conflicts_known(pr: &GithubPrDto) -> Option<bool> {
    if has_conflicts(pr) {
        Some(true)
    } else if pr.mergeable == Mergeability::Unknown {
        None
    } else {
        Some(false)
    }
}

/// Whether the pull request is ready to merge, or `None` while GitHub has not computed the
/// merge state.
pub(crate) fn ready_known(pr: &GithubPrDto) -> Option<bool> {
    (pr.merge_state != MergeState::Unknown).then(|| classify(pr) == Some(MergeKind::Ready))
}

fn has_conflicts(pr: &GithubPrDto) -> bool {
    pr.mergeable == Mergeability::Conflicting || pr.merge_state == MergeState::Dirty
}

fn review_explains(pr: &GithubPrDto) -> bool {
    matches!(
        pr.review_decision,
        Some(ReviewDecision::ReviewRequired | ReviewDecision::ChangesRequested)
    )
}

fn ci_explains(pr: &GithubPrDto) -> bool {
    !matches!(pr.ci, CiState::Success | CiState::None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::GithubMergeQueueDto;

    fn pr() -> GithubPrDto {
        GithubPrDto {
            id: "PR_1".into(),
            number: 1,
            title: "T".into(),
            url: "https://github.com/acme/app/pull/1".into(),
            repo: "acme/app".into(),
            author: "octocat".into(),
            is_draft: false,
            review_decision: None,
            ci: CiState::Success,
            head_ref: "h".into(),
            base_ref: "main".into(),
            updated_at: "2026-08-24T19:00:00Z".into(),
            review_request: None,
            mergeable: Mergeability::Mergeable,
            merge_state: MergeState::Unknown,
            merge_queue: None,
            auto_merge: false,
            merge_kind: None,
        }
    }

    fn with(f: impl FnOnce(&mut GithubPrDto)) -> GithubPrDto {
        let mut pr = pr();
        f(&mut pr);
        pr
    }

    fn queued() -> Option<GithubMergeQueueDto> {
        Some(GithubMergeQueueDto { position: Some(2) })
    }

    #[test]
    fn conflicts_beat_everything_whichever_field_reports_them() {
        let conflicting = with(|pr| {
            pr.mergeable = Mergeability::Conflicting;
            pr.merge_state = MergeState::Clean;
            pr.merge_queue = queued();
            pr.auto_merge = true;
        });
        assert_eq!(classify(&conflicting), Some(MergeKind::Conflicts));
        let dirty = with(|pr| pr.merge_state = MergeState::Dirty);
        assert_eq!(classify(&dirty), Some(MergeKind::Conflicts));
        // A draft with conflicts: the state says DRAFT, only `mergeable` knows.
        let draft = with(|pr| {
            pr.is_draft = true;
            pr.mergeable = Mergeability::Conflicting;
            pr.merge_state = MergeState::Draft;
        });
        assert_eq!(classify(&draft), Some(MergeKind::Conflicts));
    }

    #[test]
    fn the_queue_beats_auto_merge_which_beats_ready() {
        let both = with(|pr| {
            pr.merge_state = MergeState::Clean;
            pr.merge_queue = queued();
            pr.auto_merge = true;
        });
        assert_eq!(classify(&both), Some(MergeKind::Queued));
        let auto = with(|pr| {
            pr.merge_state = MergeState::Clean;
            pr.auto_merge = true;
        });
        assert_eq!(classify(&auto), Some(MergeKind::AutoMerge));
        let auto_blocked = with(|pr| {
            pr.merge_state = MergeState::Blocked;
            pr.review_decision = Some(ReviewDecision::ReviewRequired);
            pr.auto_merge = true;
        });
        assert_eq!(classify(&auto_blocked), Some(MergeKind::AutoMerge));
    }

    #[test]
    fn ready_is_a_clean_non_draft_and_behind_is_behind() {
        let clean = with(|pr| pr.merge_state = MergeState::Clean);
        assert_eq!(classify(&clean), Some(MergeKind::Ready));
        // HAS_HOOKS already maps to Clean in the parser; a draft is never ready.
        let draft = with(|pr| {
            pr.merge_state = MergeState::Clean;
            pr.is_draft = true;
        });
        assert_eq!(classify(&draft), None);
        let behind = with(|pr| pr.merge_state = MergeState::Behind);
        assert_eq!(classify(&behind), Some(MergeKind::Behind));
    }

    #[test]
    fn blocked_speaks_only_when_neither_the_review_nor_ci_explains_it() {
        use ReviewDecision::{Approved, ChangesRequested, ReviewRequired};
        let decisions = [
            None,
            Some(Approved),
            Some(ChangesRequested),
            Some(ReviewRequired),
        ];
        let cis = [
            CiState::Success,
            CiState::None,
            CiState::Pending,
            CiState::Failure,
            CiState::Error,
        ];
        for decision in decisions {
            for ci in cis {
                let blocked = with(|pr| {
                    pr.merge_state = MergeState::Blocked;
                    pr.review_decision = decision;
                    pr.ci = ci;
                });
                let review_quiet = matches!(decision, None | Some(Approved));
                let ci_quiet = matches!(ci, CiState::Success | CiState::None);
                let expected = (review_quiet && ci_quiet).then_some(MergeKind::Blocked);
                assert_eq!(classify(&blocked), expected, "{decision:?} × {ci:?}");
            }
        }
    }

    #[test]
    fn unknown_unstable_and_draft_states_say_nothing() {
        for state in [MergeState::Unknown, MergeState::Unstable, MergeState::Draft] {
            assert_eq!(
                classify(&with(|pr| pr.merge_state = state)),
                None,
                "{state:?}"
            );
        }
        let unknown = with(|pr| pr.mergeable = Mergeability::Unknown);
        assert_eq!(classify(&unknown), None);
    }

    #[test]
    fn the_monitor_facts_are_none_only_while_github_has_not_computed_them() {
        assert_eq!(
            conflicts_known(&with(|pr| pr.mergeable = Mergeability::Unknown)),
            None
        );
        assert_eq!(conflicts_known(&pr()), Some(false));
        assert_eq!(
            conflicts_known(&with(|pr| pr.mergeable = Mergeability::Conflicting)),
            Some(true)
        );
        // Dirty is conflicts even before `mergeable` is computed.
        assert_eq!(
            conflicts_known(&with(|pr| {
                pr.mergeable = Mergeability::Unknown;
                pr.merge_state = MergeState::Dirty;
            })),
            Some(true)
        );

        assert_eq!(ready_known(&pr()), None, "state unknown");
        assert_eq!(
            ready_known(&with(|pr| pr.merge_state = MergeState::Clean)),
            Some(true)
        );
        assert_eq!(
            ready_known(&with(|pr| pr.merge_state = MergeState::Blocked)),
            Some(false)
        );
        assert_eq!(
            ready_known(&with(|pr| {
                pr.merge_state = MergeState::Clean;
                pr.merge_queue = queued();
            })),
            Some(false),
            "the queue owns it"
        );
        assert_eq!(
            ready_known(&with(|pr| {
                pr.merge_state = MergeState::Clean;
                pr.is_draft = true;
            })),
            Some(false)
        );
    }
}
