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
