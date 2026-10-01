use crate::dto::{CiState, GithubPrDto, MergeKind, MergeState, Mergeability, ReviewDecision};

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
        MergeState::Clean if !pr.is_draft => Some(MergeKind::Ready),
        MergeState::Behind => Some(MergeKind::Behind),
        MergeState::Blocked if !review_explains(pr) && !ci_explains(pr) => Some(MergeKind::Blocked),
        _ => None,
    }
}

pub(crate) fn conflicts_known(pr: &GithubPrDto) -> Option<bool> {
    if has_conflicts(pr) {
        Some(true)
    } else if pr.mergeable == Mergeability::Unknown {
        None
    } else {
        Some(false)
    }
}

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
