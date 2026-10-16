use super::model::frame::{CellData, Notice, PrCell, RailData};
use super::model::{workspace_share_wording, InnerRing, NotchProvider, NotchSnapshot, ShowMode};
use super::sessions::LiveSession;
use crate::dto::{AgentId, LimitWindowDto};
use serde::{Deserialize, Serialize};

pub const MAX_MESSAGE: usize = 262_144;
pub const PROTOCOL_VERSION: u64 = 7;

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
    cells: Vec<MessageCell<'a>>,
    action_error: &'a Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum MessageCell<'a> {
    Provider(Box<MessageProvider<'a>>),
    PullRequests(&'a PrCell),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageProvider<'a> {
    provider: AgentId,
    #[serde(skip_serializing_if = "Option::is_none")]
    reading: Option<NativeReading>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notice: Option<&'a Notice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    paused: Option<&'a Notice>,
    sessions: &'a [LiveSession],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeReading {
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

impl From<&NotchProvider> for NativeReading {
    fn from(reading: &NotchProvider) -> Self {
        Self {
            windows: reading.windows.clone(),
            headline_window_id: reading.headline_window_id().map(str::to_owned),
            inner_ring: reading.inner_ring().cloned(),
            workspace_credits: reading.workspace_credits.as_ref().map(|share| {
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

impl<'a> From<&'a CellData> for MessageCell<'a> {
    fn from(cell: &'a CellData) -> Self {
        match cell {
            CellData::Provider(cell) => Self::Provider(Box::new(MessageProvider {
                provider: cell.provider,
                reading: cell.reading.as_ref().map(NativeReading::from),
                notice: cell.notice.as_ref(),
                paused: cell.paused.as_ref(),
                sessions: &cell.sessions,
            })),
            CellData::PullRequests(cell) => Self::PullRequests(cell),
        }
    }
}

pub fn encode(
    sequence: u64,
    snapshot: &NotchSnapshot,
    frame: &RailData,
) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&Message {
        version: PROTOCOL_VERSION,
        sequence,
        snapshot,
        cells: frame.cells.iter().map(MessageCell::from).collect(),
        action_error: &frame.action_error,
    })
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
