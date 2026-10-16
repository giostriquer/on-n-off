use super::host::{CellData, PrCellData, PrListData, PrRowData, ProviderData, RailData};
use super::model::{
    workspace_share_wording, GithubList, InnerRing, NotchProvider, NotchSnapshot, PrContext,
    ShowMode,
};
use super::sessions::LiveSession;
use crate::dto::{
    AgentId, CiState, GithubStatus, LimitWindowDto, LimitsStatus, MergeKind, ReviewDecision,
};
use serde::{Deserialize, Serialize};

pub const MAX_MESSAGE: usize = 262_144;
pub const PROTOCOL_VERSION: u64 = 6;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
    Ready,
    Ack { sequence: u64 },
    ScreensChanged,
    Refresh,
    OpenLimits,
    OpenPullRequests,
    SetShow { show: ShowMode },
}

pub fn decode_event(line: &[u8]) -> Result<Action, String> {
    if line.len() > MAX_MESSAGE {
        return Err("Native notch message is too large.".into());
    }
    let mut value: serde_json::Value =
        serde_json::from_slice(line).map_err(|_| "Invalid native notch message.")?;
    let object = value
        .as_object_mut()
        .ok_or("Invalid native notch message.")?;
    if object.remove("version").and_then(|v| v.as_u64()) != Some(PROTOCOL_VERSION) {
        return Err("Unsupported native notch protocol.".into());
    }
    let fields = object.len();
    let action: Action =
        serde_json::from_value(value).map_err(|_| "Invalid native notch action.")?;
    if matches!(
        action,
        Action::Ready
            | Action::ScreensChanged
            | Action::Refresh
            | Action::OpenLimits
            | Action::OpenPullRequests
    ) && fields != 1
    {
        return Err("Unexpected native notch action fields.".into());
    }
    Ok(action)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Message<'a> {
    version: u64,
    sequence: u64,
    snapshot: &'a NotchSnapshot,
    providers: Vec<MessageProvider<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pull_requests: Option<NativePullRequests>,
    action_error: &'a Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageProvider<'a> {
    #[serde(flatten)]
    entry: NativeProvider,
    sessions: &'a [LiveSession],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeProvider {
    provider: AgentId,
    status: LimitsStatus,
    current_account: bool,
    message: Option<String>,
    windows: Vec<LimitWindowDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    headline_window_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inner_ring: Option<InnerRing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_credits: Option<NativeWorkspaceCredits>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeWorkspaceCredits {
    used_percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    resets_at: Option<String>,
    left: String,
    renewed: String,
}

impl From<&NotchProvider> for NativeProvider {
    fn from(cell: &NotchProvider) -> Self {
        Self {
            provider: cell.provider,
            status: cell.status,
            current_account: true,
            message: cell.message.clone(),
            windows: cell.windows.clone(),
            headline_window_id: cell.headline_window_id().map(str::to_owned),
            inner_ring: cell.inner_ring().cloned(),
            workspace_credits: cell.workspace_credits.as_ref().map(|share| {
                let wording = workspace_share_wording(share);
                NativeWorkspaceCredits {
                    used_percent: share.used_percent,
                    resets_at: share.resets_at.clone(),
                    left: wording.left,
                    renewed: wording.renewed,
                }
            }),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativePullRequests {
    status: GithubStatus,
    hint: Option<String>,
    stale: bool,
    lists: Vec<NativePrList>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativePrList {
    id: GithubList,
    total: u64,
    items: Vec<NativePr>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativePr {
    id: String,
    number: u64,
    title: String,
    url: String,
    repo: String,
    author: String,
    is_draft: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    review_decision: Option<ReviewDecision>,
    ci: CiState,
    #[serde(skip_serializing_if = "Option::is_none")]
    merge_kind: Option<MergeKind>,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<PrContext>,
}

impl From<&PrCellData> for NativePullRequests {
    fn from(cell: &PrCellData) -> Self {
        Self {
            status: cell.status,
            hint: cell.hint.clone(),
            stale: cell.stale,
            lists: cell.lists.iter().map(NativePrList::from).collect(),
        }
    }
}

impl From<&PrListData> for NativePrList {
    fn from(list: &PrListData) -> Self {
        Self {
            id: list.id,
            total: list.total,
            items: list.items.iter().map(NativePr::from).collect(),
        }
    }
}

impl From<&PrRowData> for NativePr {
    fn from(row: &PrRowData) -> Self {
        Self {
            id: row.id.clone(),
            number: row.number,
            title: row.title.clone(),
            url: row.url.clone(),
            repo: row.repo.clone(),
            author: row.author.clone(),
            is_draft: row.is_draft,
            review_decision: row.review_decision,
            ci: row.ci,
            merge_kind: row.merge_kind,
            updated_at: row.updated_at.clone(),
            context: row.context,
        }
    }
}

pub fn encode(
    sequence: u64,
    snapshot: &NotchSnapshot,
    frame: &RailData,
) -> Result<Vec<u8>, String> {
    let providers = frame
        .cells
        .iter()
        .filter_map(|cell| match cell {
            CellData::Provider(ProviderData { cell, sessions }) => Some(MessageProvider {
                entry: NativeProvider::from(cell),
                sessions,
            }),
            CellData::PullRequests(_) => None,
        })
        .collect();
    let pull_requests = frame.cells.iter().find_map(|cell| match cell {
        CellData::PullRequests(cell) => Some(NativePullRequests::from(cell)),
        CellData::Provider(_) => None,
    });
    serde_json::to_vec(&Message {
        version: PROTOCOL_VERSION,
        sequence,
        snapshot,
        providers,
        pull_requests,
        action_error: &frame.action_error,
    })
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
