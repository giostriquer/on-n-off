use super::*;
use crate::dto::GithubStatus;
use crate::side_notch::host::frame::PrListData;
use crate::side_notch::model::GithubList;

#[test]
fn accepts_only_versioned_typed_actions() {
    assert_eq!(
        decode_event(br#"{"version":6,"type":"ready"}"#),
        Ok(Action::Ready)
    );
    assert_eq!(
        decode_event(br#"{"version":6,"type":"ack","sequence":42}"#),
        Ok(Action::Ack { sequence: 42 })
    );
    assert_eq!(
        decode_event(br#"{"version":6,"type":"openPullRequests"}"#),
        Ok(Action::OpenPullRequests)
    );
    assert_eq!(
        decode_event(br#"{"version":6,"type":"setShow","show":"onHover"}"#),
        Ok(Action::SetShow {
            show: ShowMode::OnHover
        })
    );
    assert!(decode_event(br#"{"version":6,"type":"setShow","show":"sometimes"}"#).is_err());
}

#[test]
fn rejects_other_protocols_commands_and_unbounded_messages() {
    for line in [
        br#"{"version":1,"type":"ready"}"#.as_slice(),
        br#"{"version":2,"type":"ready"}"#,
        br#"{"version":3,"type":"ready"}"#,
        br#"{"version":4,"type":"ready"}"#,
        br#"{"version":5,"type":"ready"}"#,
        br#"{"version":7,"type":"ready"}"#,
        br#"{"version":6,"type":"exec","command":"arbitrary"}"#,
        br#"{"type":"refresh"}"#,
        br#"{"version":6,"type":"openLimits","path":"arbitrary"}"#,
        br#"{"version":6,"type":"save","revision":0,"request":1,"settings":{"enabled":true,"displayId":"external","edge":"left","size":"standard"}}"#,
    ] {
        assert!(decode_event(line).is_err());
    }
    assert!(decode_event(&vec![b' '; MAX_MESSAGE + 1]).is_err());
}

fn encoded(cells: Vec<CellData>) -> serde_json::Value {
    let snapshot = NotchSnapshot {
        revision: 0,
        supported: true,
        liquid_glass: false,
        settings: crate::side_notch::model::NotchSettings::default(),
        displays: Vec::new(),
        error: None,
    };
    let frame = RailData {
        settings: snapshot.settings.clone(),
        cells,
        action_error: None,
    };
    serde_json::from_slice(&encode(1, &snapshot, &frame).unwrap()).unwrap()
}

fn sent_provider(entries: serde_json::Value) -> serde_json::Value {
    let cell = NotchProvider::current(serde_json::from_value(entries).unwrap()).unwrap();
    encoded(vec![CellData::Provider(ProviderData {
        cell,
        sessions: Vec::new(),
    })])["providers"][0]
        .clone()
}

#[test]
fn sends_only_the_current_account_and_omits_account_identifiers() {
    let payload = sent_provider(serde_json::json!([
        {"provider":"claude","status":"ok","currentAccount":false,"plan":"remembered-plan","windows":[]},
        {"provider":"claude","status":"signedOut","currentAccount":true,"account":{"id":"private-id","label":"private-label"},"plan":"max","windows":[]}
    ]));
    assert_eq!(payload["status"], "signedOut");
    assert!(payload.get("account").is_none());
    assert!(
        payload.get("plan").is_none(),
        "the helper draws no plan, so none is sent"
    );
    assert!(payload.get("credits").is_none());
    assert!(payload.get("workspaceCredits").is_none());
    assert_eq!(payload["provider"], "claude");
    assert_eq!(payload["sessions"], serde_json::json!([]));
    assert!(payload.get("headlineWindowId").is_none());
    assert!(payload.get("innerRing").is_none());
    assert!(NotchProvider::current(Vec::new()).is_none());
}

#[test]
fn the_headline_and_the_inner_ring_travel_as_window_ids() {
    let payload = sent_provider(serde_json::json!([
        {"provider":"claude","status":"ok","currentAccount":true,"windows":[
            {"id":"weekly_all","label":"Weekly · all models","kind":"weekly","usedPercent":7.0,"observedAt":"2026-09-01T10:00:00Z"},
            {"id":"weekly_scoped:Fable","label":"Weekly · Fable","kind":"model","usedPercent":13.0,"observedAt":"2026-09-01T10:00:00Z"}
        ]}
    ]));
    assert_eq!(payload["headlineWindowId"], "weekly_all");
    assert_eq!(
        payload["innerRing"],
        serde_json::json!({"kind":"fable","windowId":"weekly_scoped:Fable"})
    );
}

#[test]
fn a_business_members_credit_share_travels_worded_with_the_readers_meter() {
    let payload = sent_provider(serde_json::json!([
        {"provider":"codex","status":"ok","currentAccount":true,"windows":[],
         "workspaceCredits":{"limit":"25000","used":"8000","usedPercent":40.0,"resetsAt":"2026-10-01T12:00:00+00:00","reached":false}}
    ]));
    assert_eq!(
        payload["workspaceCredits"],
        serde_json::json!({"usedPercent":40.0,"resetsAt":"2026-10-01T12:00:00+00:00",
            "left":"17,000 of 25,000 left","renewed":"25,000 of 25,000 left"})
    );
    assert_eq!(
        payload["innerRing"],
        serde_json::json!({"kind":"workspaceShare"})
    );
}

#[test]
fn pull_requests_travel_only_with_a_pull_request_cell() {
    let message = encoded(Vec::new());
    assert_eq!(message["version"], PROTOCOL_VERSION);
    assert_eq!(message["sequence"], 1);
    assert!(message.get("pullRequests").is_none());
    assert_eq!(message["providers"], serde_json::json!([]));

    let message = encoded(vec![CellData::PullRequests(PrCellData {
        status: GithubStatus::RateLimited,
        hint: None,
        stale: false,
        lists: vec![PrListData {
            id: GithubList::Assigned,
            total: 0,
            items: Vec::new(),
        }],
    })]);
    assert_eq!(
        message["pullRequests"],
        serde_json::json!({"status":"rateLimited","hint":null,"stale":false,
            "lists":[{"id":"assigned","total":0,"items":[]}]})
    );
}
