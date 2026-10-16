use super::{GithubList, NotchProvider, NotchSettings, PrContext};
use crate::dto::{
    AgentId, CiState, GithubPrDto, GithubPrsDto, GithubStatus, LimitsStatus, MergeKind,
    ReviewDecision,
};
use crate::side_notch::sessions::LiveSession;
use serde::Serialize;

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

#[derive(Clone, Debug, PartialEq)]
pub struct PrCell {
    pub reading: Option<PrReading>,
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
    pub status: GithubStatus,
    pub hint: Option<String>,
    pub stale: bool,
    #[serde(skip)]
    pub readable: bool,
    #[serde(skip)]
    pub count: u64,
    #[serde(skip)]
    pub segments: Vec<PrSegment>,
    pub lists: Vec<PrList>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrSegment {
    pub ci: CiState,
    pub passing_with_conflicts: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrList {
    pub id: GithubList,
    #[serde(skip)]
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
    pub is_draft: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_decision: Option<ReviewDecision>,
    pub ci: CiState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_kind: Option<MergeKind>,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<PrContext>,
    #[serde(skip)]
    pub badges: Vec<Badge>,
    #[serde(skip)]
    pub tag: Option<Badge>,
}

pub fn rail(
    settings: &NotchSettings,
    read: impl Fn(AgentId) -> (Option<NotchProvider>, Vec<LiveSession>),
    pulls: Option<&GithubPrsDto>,
    action_error: Option<String>,
) -> RailData {
    let mut cells: Vec<CellData> = settings
        .rail_providers()
        .into_iter()
        .map(|provider| {
            let (reading, sessions) = read(provider);
            CellData::Provider(provider_cell(provider, reading, sessions))
        })
        .collect();
    if settings.pull_requests.enabled {
        cells.push(CellData::PullRequests(pr_cell(
            pulls,
            &settings.pull_requests.selected_lists(),
        )));
    }
    RailData {
        settings: settings.clone(),
        cells,
        action_error,
    }
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
        Some(reading) if !reading.windows.is_empty() || reading.message.is_some() => (None, None),
        _ => (Some(muted("Checking usage…".into())), None),
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
        reading: Some(reading),
        notice,
    }
}

fn pr_reading(dto: &GithubPrsDto, selected: &[GithubList]) -> PrReading {
    let readable = dto.status == GithubStatus::Ok || dto.stale;
    let lists: Vec<PrList> = selected
        .iter()
        .filter(|_| readable)
        .map(|list| {
            let source = match list {
                GithubList::Mine => &dto.data.mine,
                GithubList::ReviewRequested => &dto.data.review_requested,
                GithubList::Assigned => &dto.data.assigned,
            };
            PrList {
                id: *list,
                title: list_title(*list),
                total: source.total,
                items: source
                    .items
                    .iter()
                    .filter_map(pr_row)
                    .take(MAX_PULL_REQUESTS)
                    .collect(),
            }
        })
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    let rows: Vec<&PrRow> = lists
        .iter()
        .flat_map(|list| &list.items)
        .filter(|row| seen.insert(row.id.as_str()))
        .collect();
    let count = rows.len() as u64;
    let segments = if readable {
        rows.iter()
            .take(MAX_RING_SEGMENTS)
            .map(|row| PrSegment {
                ci: row.ci,
                passing_with_conflicts: row.ci == CiState::Success
                    && row.merge_kind == Some(MergeKind::Conflicts),
            })
            .collect()
    } else {
        Vec::new()
    };
    PrReading {
        status: dto.status,
        hint: dto.hint.clone(),
        stale: dto.stale,
        readable,
        count,
        segments,
        lists,
    }
}

fn pr_row(pr: &GithubPrDto) -> Option<PrRow> {
    if !pr.url.starts_with("https://github.com/") {
        return None;
    }
    Some(PrRow {
        id: pr.id.clone(),
        number: pr.number,
        title: pr.title.clone(),
        url: pr.url.clone(),
        repo: pr.repo.clone(),
        author: pr.author.clone(),
        is_draft: pr.is_draft,
        review_decision: pr.review_decision,
        ci: pr.ci,
        merge_kind: pr.merge_kind,
        updated_at: pr.updated_at.clone(),
        context: PrContext::of(pr),
        badges: badges(pr),
        tag: PrContext::of(pr).map(tag),
    })
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
