use super::{GithubList, NotchProvider, NotchSettings, PrContext};
use crate::dto::{
    AgentId, CiState, GithubPrDto, GithubPrsDto, GithubStatus, LimitsStatus, MergeKind,
    ReviewDecision,
};
use crate::side_notch::sessions::LiveSession;
use serde::Serialize;
use std::collections::HashMap;

pub const MAX_PULL_REQUESTS: usize = 25;
pub const MAX_RING_SEGMENTS: usize = 24;

#[derive(Clone, Debug, PartialEq)]
pub struct RailData {
    pub settings: NotchSettings,
    pub cells: Vec<CellData>,
    pub action_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CellData {
    Provider(ProviderCell),
    PullRequests(PrCell),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderCell {
    pub provider: AgentId,
    pub reading: Option<NotchProvider>,
    pub notice: Option<Notice>,
    pub paused: Option<Notice>,
    pub sessions: Vec<LiveSession>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PrCell {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reading: Option<PrReading>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<Notice>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    Muted,
    Green,
    Red,
    Amber,
    Work,
    Personal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub text: String,
    pub tone: Tone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Badge {
    pub label: &'static str,
    pub tone: Tone,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrReading {
    pub readable: bool,
    pub count: u64,
    pub segments: Vec<PrSegment>,
    pub attention: PrAttention,
    pub lists: Vec<PrList>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrSegment {
    pub ci: CiState,
    pub passing_with_conflicts: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrAttention {
    pub conflicts: u64,
    pub failing: u64,
    pub changes_requested: u64,
    pub ready: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrList {
    pub id: GithubList,
    pub title: &'static str,
    pub total: u64,
    pub items: Vec<PrRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrRow {
    pub id: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub repo: String,
    pub author: String,
    pub ci: CiState,
    pub updated_at: String,
    pub badges: Vec<Badge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<Badge>,
}

pub fn rail(
    settings: &NotchSettings,
    reads: impl IntoIterator<Item = (AgentId, Option<NotchProvider>, Vec<LiveSession>)>,
    pulls: Option<&GithubPrsDto>,
    action_error: Option<String>,
) -> RailData {
    let mut reads: HashMap<AgentId, (Option<NotchProvider>, Vec<LiveSession>)> = reads
        .into_iter()
        .map(|(provider, reading, sessions)| (provider, (reading, sessions)))
        .collect();
    let cells = slots(settings)
        .map(|slot| match slot {
            Slot::Provider(provider) => {
                let (reading, sessions) = reads.remove(&provider).unwrap_or_default();
                CellData::Provider(provider_cell(provider, reading, sessions))
            }
            Slot::PullRequests => {
                CellData::PullRequests(pr_cell(pulls, &settings.pull_requests.selected_lists()))
            }
        })
        .collect();
    RailData {
        settings: settings.clone(),
        cells,
        action_error,
    }
}

pub fn cell_count(settings: &NotchSettings) -> usize {
    slots(settings).count()
}

enum Slot {
    Provider(AgentId),
    PullRequests,
}

fn slots(settings: &NotchSettings) -> impl Iterator<Item = Slot> {
    settings
        .rail_providers()
        .into_iter()
        .map(Slot::Provider)
        .chain(settings.pull_requests.enabled.then_some(Slot::PullRequests))
}

fn provider_cell(
    provider: AgentId,
    reading: Option<NotchProvider>,
    sessions: Vec<LiveSession>,
) -> ProviderCell {
    let (notice, paused) = match &reading {
        Some(reading) if reading.status != LimitsStatus::Ok => (
            Some(muted(
                reading
                    .message
                    .clone()
                    .unwrap_or_else(|| "Usage unavailable.".into()),
            )),
            (!reading.windows.is_empty() || reading.workspace_credits.is_some()).then(|| Notice {
                text: "Refresh paused. Last observed values below.".into(),
                tone: Tone::Amber,
            }),
        ),
        Some(_) => (None, None),
        None => (Some(muted("Checking usage…".into())), None),
    };
    ProviderCell {
        provider,
        reading,
        notice,
        paused,
        sessions,
    }
}

fn muted(text: String) -> Notice {
    Notice {
        text,
        tone: Tone::Muted,
    }
}

fn pr_cell(dto: Option<&GithubPrsDto>, selected: &[GithubList]) -> PrCell {
    let Some(dto) = dto else {
        return PrCell {
            label: "—".into(),
            reading: None,
            notice: Some(muted("Checking pull requests…".into())),
        };
    };
    let reading = pr_reading(dto, selected);
    let notice = dto
        .hint
        .clone()
        .filter(|_| dto.status != GithubStatus::Ok)
        .map(|text| Notice {
            text,
            tone: if reading.readable {
                Tone::Amber
            } else {
                Tone::Muted
            },
        });
    PrCell {
        label: if reading.readable {
            reading.count.to_string()
        } else {
            "—".into()
        },
        reading: Some(reading),
        notice,
    }
}

fn pr_reading(dto: &GithubPrsDto, selected: &[GithubList]) -> PrReading {
    let readable = dto.status == GithubStatus::Ok || dto.stale;
    let shown: Vec<(GithubList, u64, Vec<&GithubPrDto>)> = if readable {
        selected
            .iter()
            .map(|list| {
                let source = match list {
                    GithubList::Mine => &dto.data.mine,
                    GithubList::ReviewRequested => &dto.data.review_requested,
                    GithubList::Assigned => &dto.data.assigned,
                };
                let items = source
                    .items
                    .iter()
                    .filter(|pr| pr.url.starts_with("https://github.com/"))
                    .take(MAX_PULL_REQUESTS)
                    .collect();
                (*list, source.total, items)
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut seen = std::collections::BTreeSet::new();
    let rows: Vec<&GithubPrDto> = shown
        .iter()
        .flat_map(|(_, _, items)| items.iter().copied())
        .filter(|pr| seen.insert(pr.id.as_str()))
        .collect();
    let count = |test: fn(&GithubPrDto) -> bool| rows.iter().filter(|pr| test(pr)).count() as u64;
    PrReading {
        readable,
        count: rows.len() as u64,
        segments: rows
            .iter()
            .take(MAX_RING_SEGMENTS)
            .map(|pr| PrSegment {
                ci: pr.ci,
                passing_with_conflicts: pr.ci == CiState::Success
                    && pr.merge_kind == Some(MergeKind::Conflicts),
            })
            .collect(),
        attention: PrAttention {
            conflicts: count(|pr| pr.merge_kind == Some(MergeKind::Conflicts)),
            failing: count(|pr| matches!(pr.ci, CiState::Failure | CiState::Error)),
            changes_requested: count(|pr| {
                pr.review_decision == Some(ReviewDecision::ChangesRequested)
            }),
            ready: count(|pr| pr.merge_kind == Some(MergeKind::Ready)),
        },
        lists: shown
            .into_iter()
            .map(|(id, total, items)| PrList {
                id,
                title: list_title(id),
                total,
                items: items.into_iter().map(pr_row).collect(),
            })
            .collect(),
    }
}

fn pr_row(pr: &GithubPrDto) -> PrRow {
    PrRow {
        id: pr.id.clone(),
        number: pr.number,
        title: pr.title.clone(),
        url: pr.url.clone(),
        repo: pr.repo.clone(),
        author: pr.author.clone(),
        ci: pr.ci,
        updated_at: pr.updated_at.clone(),
        badges: badges(pr),
        tag: PrContext::of(pr).map(tag),
    }
}

fn list_title(list: GithubList) -> &'static str {
    match list {
        GithubList::Mine => "Mine",
        GithubList::ReviewRequested => "Review requested",
        GithubList::Assigned => "Assigned",
    }
}

fn badges(pr: &GithubPrDto) -> Vec<Badge> {
    let badge = |label, tone| Badge { label, tone };
    let draft = pr.is_draft.then(|| badge("Draft", Tone::Muted));
    let review = match pr.review_decision {
        Some(ReviewDecision::Approved) => Some(badge("Approved", Tone::Green)),
        Some(ReviewDecision::ChangesRequested) => Some(badge("Changes requested", Tone::Red)),
        Some(ReviewDecision::ReviewRequired) | None => None,
    };
    let merge = pr.merge_kind.map(|kind| match kind {
        MergeKind::Conflicts => badge("Conflicts", Tone::Red),
        MergeKind::Queued => badge("Queued", Tone::Green),
        MergeKind::AutoMerge => badge("Auto-merge", Tone::Muted),
        MergeKind::Ready => badge("Ready to merge", Tone::Green),
        MergeKind::Behind => badge("Behind base", Tone::Amber),
        MergeKind::Blocked => badge("Blocked", Tone::Amber),
    });
    [draft, review, merge].into_iter().flatten().collect()
}

fn tag(context: PrContext) -> Badge {
    match context {
        PrContext::Work => Badge {
            label: "Work",
            tone: Tone::Work,
        },
        PrContext::Personal => Badge {
            label: "Personal",
            tone: Tone::Personal,
        },
    }
}

#[cfg(test)]
mod tests;
