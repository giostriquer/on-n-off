use crate::dto::{
    CiState, GithubPrDto, GithubPrsData, MergeKind, MergeState, Mergeability, ReviewDecision,
};

pub(super) fn observed(data: &GithubPrsData) -> Vec<GithubPrDto> {
    open_prs(data)
        .filter(|pr| conflicts_known(pr).is_some())
        .cloned()
        .collect()
}

fn open_prs(data: &GithubPrsData) -> impl Iterator<Item = &GithubPrDto> {
    data.mine
        .items
        .iter()
        .chain(&data.review_requested.items)
        .chain(&data.assigned.items)
}

pub(super) fn apply_observed(pr: &mut GithubPrDto, observed: &[GithubPrDto]) -> bool {
    let Some(known) = observed.iter().find(|known| same_revision(pr, known)) else {
        return false;
    };
    pr.mergeable = known.mergeable;
    pr.merge_state = known.merge_state;
    pr.merge_kind = classify(pr);
    true
}

pub(super) fn keep_conflicts(data: &mut GithubPrsData, previous: Option<&GithubPrsData>) {
    let observed = observed(data);
    let retained: Vec<_> = open_prs(data)
        .filter(|pr| pr.merge_kind == Some(MergeKind::Conflicts))
        .cloned()
        .collect();
    let previous = previous.filter(|previous| {
        data.viewer.as_ref().is_some_and(|viewer| {
            previous
                .viewer
                .as_ref()
                .is_some_and(|old| viewer.eq_ignore_ascii_case(old))
        })
    });
    for pr in data
        .mine
        .items
        .iter_mut()
        .chain(&mut data.review_requested.items)
        .chain(&mut data.assigned.items)
    {
        if conflicts_known(pr).is_none() && !apply_observed(pr, &observed) {
            if let Some(old) = retained
                .iter()
                .find(|old| same_revision(pr, old))
                .or_else(|| {
                    previous.and_then(|data| open_prs(data).find(|old| same_revision(pr, old)))
                })
            {
                keep_conflict(pr, old);
            }
        }
    }
}

fn same_revision(pr: &GithubPrDto, previous: &GithubPrDto) -> bool {
    pr.id == previous.id
        && pr.base_ref == previous.base_ref
        && pr
            .head_ref_oid
            .as_ref()
            .is_some_and(|head| !head.is_empty() && previous.head_ref_oid.as_ref() == Some(head))
}

pub(super) fn keep_conflict(pr: &mut GithubPrDto, previous: &GithubPrDto) {
    if conflicts_known(pr).is_none()
        && previous.merge_kind == Some(MergeKind::Conflicts)
        && same_revision(pr, previous)
    {
        pr.merge_kind = Some(MergeKind::Conflicts);
    }
}

pub(super) fn restore(pr: &mut GithubPrDto) {
    let remembered = pr.merge_kind == Some(MergeKind::Conflicts)
        && conflicts_known(pr).is_none()
        && pr
            .head_ref_oid
            .as_ref()
            .is_some_and(|head| !head.is_empty());
    pr.merge_kind = if remembered {
        Some(MergeKind::Conflicts)
    } else {
        classify(pr)
    };
}

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
    (pr.merge_state != MergeState::Unknown).then(|| {
        classify(pr) == Some(MergeKind::Ready) && pr.merge_kind != Some(MergeKind::Conflicts)
    })
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
