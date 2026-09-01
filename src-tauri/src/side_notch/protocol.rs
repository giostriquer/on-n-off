use serde::Deserialize;

pub const MAX_MESSAGE: usize = 262_144;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
    Ready,
    Ack { sequence: u64 },
    ScreensChanged,
    Refresh,
    OpenLimits,
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
    if object.remove("version").and_then(|v| v.as_u64()) != Some(1) {
        return Err("Unsupported native notch protocol.".into());
    }
    let fields = object.len();
    let action: Action =
        serde_json::from_value(value).map_err(|_| "Invalid native notch action.")?;
    if matches!(
        action,
        Action::Ready | Action::ScreensChanged | Action::Refresh | Action::OpenLimits
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
            decode_event(br#"{"version":1,"type":"ready"}"#),
            Ok(Action::Ready)
        );
        assert_eq!(
            decode_event(br#"{"version":1,"type":"ack","sequence":42}"#),
            Ok(Action::Ack { sequence: 42 })
        );
    }

    #[test]
    fn rejects_future_protocols_commands_and_unbounded_messages() {
        for line in [
            br#"{"version":2,"type":"ready"}"#.as_slice(),
            br#"{"version":1,"type":"exec","command":"arbitrary"}"#,
            br#"{"type":"refresh"}"#,
            br#"{"version":1,"type":"openLimits","path":"arbitrary"}"#,
            br#"{"version":1,"type":"save","revision":0,"request":1,"settings":{"enabled":true,"displayId":"external","edge":"left","size":"standard"}}"#,
        ] {
            assert!(decode_event(line).is_err());
        }
        assert!(decode_event(&vec![b' '; MAX_MESSAGE + 1]).is_err());
    }
}
