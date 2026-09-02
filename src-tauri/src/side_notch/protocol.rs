use super::model::ShowMode;
use serde::Deserialize;

pub const MAX_MESSAGE: usize = 262_144;
/// Bumped whenever the host → helper message shape changes; a stale helper must fail loudly.
pub const PROTOCOL_VERSION: u64 = 2;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
    Ready,
    Ack {
        sequence: u64,
    },
    ScreensChanged,
    Refresh,
    OpenLimits,
    OpenPullRequests,
    /// The rail's pin control: flips between always showing the rail and showing it on hover.
    SetShow {
        show: ShowMode,
    },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_versioned_typed_actions() {
        assert_eq!(
            decode_event(br#"{"version":2,"type":"ready"}"#),
            Ok(Action::Ready)
        );
        assert_eq!(
            decode_event(br#"{"version":2,"type":"ack","sequence":42}"#),
            Ok(Action::Ack { sequence: 42 })
        );
        assert_eq!(
            decode_event(br#"{"version":2,"type":"openPullRequests"}"#),
            Ok(Action::OpenPullRequests)
        );
        assert_eq!(
            decode_event(br#"{"version":2,"type":"setShow","show":"onHover"}"#),
            Ok(Action::SetShow {
                show: ShowMode::OnHover
            })
        );
        assert!(decode_event(br#"{"version":2,"type":"setShow","show":"sometimes"}"#).is_err());
    }

    #[test]
    fn rejects_other_protocols_commands_and_unbounded_messages() {
        for line in [
            br#"{"version":1,"type":"ready"}"#.as_slice(),
            br#"{"version":3,"type":"ready"}"#,
            br#"{"version":2,"type":"exec","command":"arbitrary"}"#,
            br#"{"type":"refresh"}"#,
            br#"{"version":2,"type":"openLimits","path":"arbitrary"}"#,
            br#"{"version":2,"type":"save","revision":0,"request":1,"settings":{"enabled":true,"displayId":"external","edge":"left","size":"standard"}}"#,
        ] {
            assert!(decode_event(line).is_err());
        }
        assert!(decode_event(&vec![b' '; MAX_MESSAGE + 1]).is_err());
    }
}
