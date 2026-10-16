use super::{GithubList, NotchProvider, NotchSettings, PrContext};
use crate::dto::{
    AgentId, CiState, GithubPrDto, GithubPrsDto, GithubStatus, MergeKind, ReviewDecision,
};
use crate::side_notch::sessions::LiveSession;
use serde::Serialize;

pub const MAX_PULL_REQUESTS: usize = 25;

#[derive(Clone, Debug, PartialEq)]
pub struct RailData {
    pub settings: NotchSettings,
    pub cells: Vec<CellData>,
    pub action_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CellData {
    Provider(ProviderData),
    PullRequests(PrCellData),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderData {
    pub cell: NotchProvider,
    pub sessions: Vec<LiveSession>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrCellData {
    pub status: GithubStatus,
    pub hint: Option<String>,
    pub stale: bool,
    pub lists: Vec<PrListData>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrListData {
    pub id: GithubList,
    pub total: u64,
    pub items: Vec<PrRowData>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrRowData {
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
        .filter_map(|agent| {
            let (reading, sessions) = read(agent);
            reading.map(|cell| CellData::Provider(ProviderData { cell, sessions }))
        })
        .collect();
    if settings.pull_requests.enabled {
        if let Some(dto) = pulls {
            cells.push(CellData::PullRequests(pr_cell(
                dto,
                &settings.pull_requests.selected_lists(),
            )));
        }
    }
    RailData {
        settings: settings.clone(),
        cells,
        action_error,
    }
}

fn pr_cell(dto: &GithubPrsDto, selected: &[GithubList]) -> PrCellData {
    PrCellData {
        status: dto.status,
        hint: dto.hint.clone(),
        stale: dto.stale,
        lists: selected
            .iter()
            .map(|list| {
                let source = match list {
                    GithubList::Mine => &dto.data.mine,
                    GithubList::ReviewRequested => &dto.data.review_requested,
                    GithubList::Assigned => &dto.data.assigned,
                };
                PrListData {
                    id: *list,
                    total: source.total,
                    items: source
                        .items
                        .iter()
                        .filter_map(pr_row)
                        .take(MAX_PULL_REQUESTS)
                        .collect(),
                }
            })
            .collect(),
    }
}

fn pr_row(pr: &GithubPrDto) -> Option<PrRowData> {
    if !pr.url.starts_with("https://github.com/") {
        return None;
    }
    Some(PrRowData {
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
    })
}

#[cfg(test)]
mod tests;
