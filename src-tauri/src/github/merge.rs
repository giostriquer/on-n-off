use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::snapshot::Snapshot;
use crate::dto::{
    CiState, GithubPrDto, GithubPrsData, MergeKind, MergeState, Mergeability, ReviewDecision,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct KnownAnswers {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    viewer: Option<String>,
    #[serde(default)]
    answers: Vec<Known>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Known {
    #[serde(flatten)]
    revision: Revision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    answer: Option<Answer>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Revision {
    id: String,
    head: String,
    base: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answer {
    mergeable: Mergeability,
    merge_state: MergeState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MergeFacts {
    pub(crate) conflicts: Option<bool>,
    pub(crate) ready: Option<bool>,
}

impl Revision {
    fn of(pr: &GithubPrDto) -> Option<Self> {
        let head = pr.head_ref_oid.as_deref().filter(|head| !head.is_empty())?;
        Some(Self {
            id: pr.id.clone(),
            head: head.to_string(),
            base: pr.base_ref.clone(),
        })
    }
}

impl Known {
    fn conflicts(&self) -> bool {
        self.answer.is_none_or(Answer::conflicts)
    }
}

impl Answer {
    fn of(pr: &GithubPrDto) -> Option<Self> {
        conflicts_known(pr).map(|_| Self {
            mergeable: pr.mergeable,
            merge_state: pr.merge_state,
        })
    }

    fn conflicts(self) -> bool {
        self.mergeable == Mergeability::Conflicting || self.merge_state == MergeState::Dirty
    }

    fn give(self, pr: &mut GithubPrDto) {
        pr.mergeable = self.mergeable;
        pr.merge_state = self.merge_state;
    }
}

impl KnownAnswers {
    pub(super) fn recall(
        saved: Option<Snapshot>,
        recorded: Option<(String, Vec<GithubPrDto>)>,
    ) -> Self {
        let data = match saved {
            Some(Snapshot {
                known_answers: Some(known_answers),
                ..
            }) => return known_answers,
            saved => saved.map(|saved| saved.data),
        };
        let mut known_answers = Self::default();
        if let Some(data) = &data {
            if let Some(viewer) = &data.viewer {
                known_answers.seed(viewer, data.open_prs());
            }
        }
        if let Some((viewer, open)) = &recorded {
            known_answers.seed(viewer, open);
        }
        known_answers
    }

    fn seed<'a>(&mut self, viewer: &str, shown: impl IntoIterator<Item = &'a GithubPrDto>) {
        let known_viewer = self.viewer.get_or_insert_with(|| viewer.to_string());
        if !known_viewer.eq_ignore_ascii_case(viewer) {
            return;
        }
        for pr in shown {
            let in_conflict =
                conflicts_known(pr).unwrap_or(pr.merge_kind == Some(MergeKind::Conflicts));
            if let Some(revision) = Revision::of(pr).filter(|_| in_conflict) {
                remember(
                    &mut self.answers,
                    Known {
                        revision,
                        answer: Answer::of(pr),
                    },
                );
            }
        }
    }

    pub(super) fn settle(
        &mut self,
        data: &mut GithubPrsData,
        mut unlisted: Vec<GithubPrDto>,
        earlier: &HashSet<String>,
    ) {
        let viewer = data.viewer.clone();
        let known = match (&viewer, &self.viewer) {
            (Some(viewer), Some(known)) if viewer.eq_ignore_ascii_case(known) => {
                std::mem::take(&mut self.answers)
            }
            _ => Vec::new(),
        };
        let by_revision = this_reads_answers(data.open_prs().chain(&unlisted), earlier, &known);
        let mut settled = Vec::new();
        for pr in data.open_prs_mut().chain(&mut unlisted) {
            let revision = Revision::of(pr);
            if fresh_answer(pr, earlier).is_none() {
                let answer = revision.as_ref().and_then(|revision| {
                    by_revision
                        .iter()
                        .find(|(answered, _)| answered == revision)
                        .map(|(_, answer)| *answer)
                });
                if let Some(answer) = answer {
                    answer.give(pr);
                }
            }
            let carried = revision
                .as_ref()
                .filter(|_| Answer::of(pr).is_none())
                .and_then(|revision| last_known(&known, revision))
                .filter(|entry| entry.conflicts());
            pr.merge_kind = if carried.is_some() {
                Some(MergeKind::Conflicts)
            } else {
                classify(pr)
            };
            match (revision, Answer::of(pr), carried) {
                (Some(revision), Some(answer), _) => remember(
                    &mut settled,
                    Known {
                        revision,
                        answer: Some(answer),
                    },
                ),
                (Some(revision), None, Some(entry)) => remember(
                    &mut settled,
                    Known {
                        revision,
                        answer: entry.answer,
                    },
                ),
                _ => {}
            }
        }
        *self = Self {
            answers: if viewer.is_some() {
                settled
            } else {
                Vec::new()
            },
            viewer,
        };
    }
}

fn this_reads_answers<'a>(
    rows: impl Iterator<Item = &'a GithubPrDto> + Clone,
    earlier: &HashSet<String>,
    known: &[Known],
) -> Vec<(Revision, Answer)> {
    let mut by_revision: Vec<(Revision, Answer)> = rows
        .clone()
        .filter_map(|pr| Some((Revision::of(pr)?, fresh_answer(pr, earlier)?)))
        .collect();
    for pr in rows.filter(|pr| is_earlier(pr, earlier)) {
        let Some(revision) = Revision::of(pr) else {
            continue;
        };
        if by_revision
            .iter()
            .any(|(answered, _)| *answered == revision)
        {
            continue;
        }
        let last = last_known(known, &revision).and_then(|entry| entry.answer);
        if let Some(answer) = last.or_else(|| Answer::of(pr)) {
            by_revision.push((revision, answer));
        }
    }
    by_revision
}

fn fresh_answer(pr: &GithubPrDto, earlier: &HashSet<String>) -> Option<Answer> {
    Answer::of(pr).filter(|_| !is_earlier(pr, earlier))
}

fn is_earlier(pr: &GithubPrDto, earlier: &HashSet<String>) -> bool {
    pr.contributed && earlier.contains(&pr.id)
}

fn last_known<'a>(known: &'a [Known], revision: &Revision) -> Option<&'a Known> {
    known.iter().find(|entry| entry.revision == *revision)
}

fn remember(answers: &mut Vec<Known>, known: Known) {
    match answers
        .iter_mut()
        .find(|entry| entry.revision == known.revision)
    {
        Some(entry) if known.conflicts() && !entry.conflicts() => *entry = known,
        Some(_) => {}
        None => answers.push(known),
    }
}

pub(crate) fn facts(pr: &GithubPrDto) -> MergeFacts {
    MergeFacts {
        conflicts: conflicts_known(pr),
        ready: (pr.merge_state != MergeState::Unknown).then(|| {
            classify(pr) == Some(MergeKind::Ready) && pr.merge_kind != Some(MergeKind::Conflicts)
        }),
    }
}

pub(super) fn classify(pr: &GithubPrDto) -> Option<MergeKind> {
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

fn conflicts_known(pr: &GithubPrDto) -> Option<bool> {
    if has_conflicts(pr) {
        Some(true)
    } else if pr.mergeable == Mergeability::Unknown {
        None
    } else {
        Some(false)
    }
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
