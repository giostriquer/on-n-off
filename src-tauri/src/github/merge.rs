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
mod tests;
