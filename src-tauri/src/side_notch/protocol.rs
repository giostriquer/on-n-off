use super::model::{Edge, NotchSettings};
use serde::Deserialize;

pub const MAX_MESSAGE: usize = 262_144;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
    Ready,
    Ack {
        sequence: u64,
    },
    ScreensChanged,
    Save {
        #[serde(deserialize_with = "native_settings")]
        settings: NotchSettings,
        revision: u64,
        request: u64,
    },
    Refresh,
    OpenLimits,
}

// Persistence tolerates omitted fields for older app versions. The helper's
// write protocol must require a complete, correctly spelled settings message.
fn native_settings<'de, D: serde::Deserializer<'de>>(input: D) -> Result<NotchSettings, D::Error> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct NativeSettings {
        enabled: bool,
        display_id: Option<String>,
        edge: Edge,
    }
    let settings = NativeSettings::deserialize(input)?;
    Ok(NotchSettings {
        enabled: settings.enabled,
        display_id: settings.display_id,
        edge: settings.edge,
    })
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
    if let Action::Save { settings, .. } = &action {
        if settings
            .display_id
            .as_ref()
            .is_some_and(|id| id.len() > 128 || id.is_empty())
        {
            return Err("Invalid display identity.".into());
        }
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
        assert_eq!(decode_event(br#"{"version":1,"type":"save","revision":0,"request":1,"settings":{"enabled":true,"displayId":"external","edge":"left"}}"#), Ok(Action::Save { revision: 0, request: 1, settings: NotchSettings { enabled: true, display_id: Some("external".into()), edge: super::super::model::Edge::Left } }));
    }

    #[test]
    fn rejects_incomplete_or_misspelled_native_settings() {
        for settings in [
            serde_json::json!({}),
            serde_json::json!({"enabled": true}),
            serde_json::json!({"edge": "left"}),
            serde_json::json!({"enabled": true, "edeg": "left"}),
            serde_json::json!({"enabled": true, "edge": "left", "extra": 1}),
        ] {
            let message = serde_json::json!({"version":1,"type":"save","revision":0,"request":1,"settings":settings});
            assert!(
                decode_event(&serde_json::to_vec(&message).unwrap()).is_err(),
                "accepted {message}"
            );
        }
    }

    #[test]
    fn rejects_future_protocols_commands_and_unbounded_messages() {
        for line in [
            br#"{"version":2,"type":"ready"}"#.as_slice(),
            br#"{"version":1,"type":"exec","command":"arbitrary"}"#,
            br#"{"type":"refresh"}"#,
            br#"{"version":1,"type":"openLimits","path":"arbitrary"}"#,
        ] {
            assert!(decode_event(line).is_err());
        }
        assert!(decode_event(&vec![b' '; MAX_MESSAGE + 1]).is_err());
    }
}
