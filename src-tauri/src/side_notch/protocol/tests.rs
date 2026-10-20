use super::*;
use crate::side_notch::model::frame;
use crate::side_notch::model::NotchSettings;
use crate::side_notch::sessions::LiveSession;

#[test]
fn accepts_only_versioned_typed_actions() {
    assert_eq!(
        decode_event(br#"{"version":7,"type":"ready"}"#),
        Ok(Action::Ready)
    );
    assert_eq!(
        decode_event(br#"{"version":7,"type":"ack","sequence":42}"#),
        Ok(Action::Ack { sequence: 42 })
    );
    assert_eq!(
        decode_event(br#"{"version":7,"type":"openPullRequests"}"#),
        Ok(Action::OpenPullRequests)
    );
    assert_eq!(
        decode_event(br#"{"version":7,"type":"setShow","show":"onHover"}"#),
        Ok(Action::SetShow {
            show: ShowMode::OnHover
        })
    );
    assert!(decode_event(br#"{"version":7,"type":"setShow","show":"sometimes"}"#).is_err());
}

#[test]
fn rejects_other_protocols_commands_and_unbounded_messages() {
    for line in [
        br#"{"version":1,"type":"ready"}"#.as_slice(),
        br#"{"version":2,"type":"ready"}"#,
        br#"{"version":3,"type":"ready"}"#,
        br#"{"version":4,"type":"ready"}"#,
        br#"{"version":5,"type":"ready"}"#,
        br#"{"version":6,"type":"ready"}"#,
        br#"{"version":8,"type":"ready"}"#,
        br#"{"version":7,"type":"exec","command":"arbitrary"}"#,
        br#"{"type":"refresh"}"#,
        br#"{"version":7,"type":"openLimits","path":"arbitrary"}"#,
        br#"{"version":7,"type":"save","revision":0,"request":1,"settings":{"enabled":true,"displayId":"external","edge":"left","size":"standard"}}"#,
    ] {
        assert!(decode_event(line).is_err());
    }
    assert!(decode_event(&vec![b' '; MAX_MESSAGE + 1]).is_err());
}

fn encoded(
    settings: NotchSettings,
    reads: Vec<(AgentId, Option<NotchProvider>, Vec<LiveSession>)>,
) -> serde_json::Value {
    let snapshot = NotchSnapshot {
        revision: 0,
        supported: true,
        liquid_glass: false,
        settings,
        displays: Vec::new(),
        error: None,
    };
    let frame = frame::rail(&snapshot.settings, reads, None, None);
    serde_json::from_slice(&encode(1, &snapshot, &frame).unwrap()).unwrap()
}

fn sent_reading(provider: AgentId, entries: serde_json::Value) -> serde_json::Value {
    let reading = NotchProvider::current(serde_json::from_value(entries).unwrap()).unwrap();
    let only = NotchSettings {
        providers: vec![provider],
        ..NotchSettings::default()
    };
    let cell = encoded(only, vec![(provider, Some(reading), Vec::new())])["cells"][0].clone();
    assert_eq!(cell["kind"], "provider");
    cell
}

#[test]
fn sends_only_the_current_account_and_omits_account_identifiers() {
    let cell = sent_reading(
        AgentId::Claude,
        serde_json::json!([
            {"provider":"claude","status":"ok","currentAccount":false,"plan":"remembered-plan","windows":[]},
            {"provider":"claude","status":"signedOut","currentAccount":true,"account":{"id":"private-id","label":"private-label"},"plan":"max","windows":[]}
        ]),
    );
    assert_eq!(cell["provider"], "claude");
    assert_eq!(cell["sessions"], serde_json::json!([]));
    assert_eq!(
        cell["notice"],
        serde_json::json!({"text":"Usage unavailable.","tone":"muted"}),
        "the signed-out read is spoken for by its notice"
    );
    let reading = &cell["reading"];
    assert!(reading.get("status").is_none());
    assert!(reading.get("message").is_none());
    assert!(reading.get("account").is_none());
    assert!(
        reading.get("plan").is_none(),
        "the helper draws no plan, so none is sent"
    );
    assert!(reading.get("currentAccount").is_none());
    assert!(reading.get("credits").is_none());
    assert!(reading.get("workspaceCredits").is_none());
    assert!(reading.get("headlineWindowId").is_none());
    assert!(reading.get("innerRing").is_none());
    assert!(NotchProvider::current(Vec::new()).is_none());
}

#[test]
fn the_headline_and_the_inner_ring_travel_as_window_ids() {
    let reading = sent_reading(AgentId::Claude, serde_json::json!([
        {"provider":"claude","status":"ok","currentAccount":true,"windows":[
            {"id":"weekly_all","label":"Weekly · all models","kind":"weekly","usedPercent":7.0,"observedAt":"2026-09-01T10:00:00Z"},
            {"id":"weekly_scoped:Fable","label":"Weekly · Fable","kind":"model","usedPercent":13.0,"observedAt":"2026-09-01T10:00:00Z"}
        ]}
    ]))["reading"]
        .clone();
    assert_eq!(reading["headlineWindowId"], "weekly_all");
    assert_eq!(
        reading["innerRing"],
        serde_json::json!({"kind":"fable","windowId":"weekly_scoped:Fable"})
    );
}

#[test]
fn a_business_members_credit_share_travels_worded_with_the_readers_meter() {
    let reading = sent_reading(AgentId::Codex, serde_json::json!([
        {"provider":"codex","status":"ok","currentAccount":true,"windows":[],
         "workspaceCredits":{"limit":"25000","used":"8000","usedPercent":40.0,"resetsAt":"2026-10-01T12:00:00+00:00","reached":false}}
    ]))["reading"]
        .clone();
    assert_eq!(
        reading["workspaceCredits"],
        serde_json::json!({"usedPercent":40.0,"resetsAt":"2026-10-01T12:00:00+00:00",
            "left":"17,000 of 25,000 left","renewed":"25,000 of 25,000 left"})
    );
    assert_eq!(
        reading["innerRing"],
        serde_json::json!({"kind":"workspaceShare"})
    );
}

#[test]
fn every_cell_travels_in_rail_order_and_a_cell_not_read_yet_carries_only_its_notice() {
    let settings = NotchSettings {
        providers: vec![AgentId::Muse, AgentId::Claude],
        ..NotchSettings::default()
    };
    let message = encoded(settings, Vec::new());
    assert_eq!(message["version"], PROTOCOL_VERSION);
    assert_eq!(PROTOCOL_VERSION, 7);
    assert_eq!(message["sequence"], 1);
    assert!(message.get("providers").is_none());
    assert!(message.get("pullRequests").is_none());
    assert_eq!(
        message["cells"],
        serde_json::json!([
            {"kind":"provider","provider":"claude",
             "notice":{"text":"Checking usage…","tone":"muted"},"sessions":[]},
            {"kind":"provider","provider":"muse",
             "notice":{"text":"Checking usage…","tone":"muted"},"sessions":[]},
            {"kind":"pullRequests","label":"—",
             "notice":{"text":"Checking pull requests…","tone":"muted"}}
        ])
    );
    assert_eq!(message["actionError"], serde_json::Value::Null);
}

#[test]
fn a_remembered_reading_out_of_range_still_reaches_the_helper_in_range() {
    use crate::dto::{
        LimitWindowKind, LimitsStatus, LimitsWorkspaceCreditsDto, ProviderLimitsDto, Reading,
    };
    let home = crate::paths::scratch_dir("notch-remembered-out-of-range");
    let read = ProviderLimitsDto::for_test(AgentId::Codex, "acct-1").with_reading(Reading {
        windows: vec![LimitWindowDto {
            id: "primary".into(),
            label: "Weekly · all models".into(),
            kind: LimitWindowKind::Weekly,
            used_percent: 50.0,
            resets_at: Some("2026-10-05T12:00:00.000Z".into()),
            window_seconds: Some(604_800),
            observed_at: "2026-10-01T10:00:00.000Z".into(),
        }],
        workspace_credits: Some(LimitsWorkspaceCreditsDto {
            limit: "100".into(),
            used: "40".into(),
            used_percent: 40.0,
            resets_at: None,
            reached: false,
        }),
        ..Reading::default()
    });
    assert!(crate::limits::remember(&home, read).saved.is_ok());
    let file = std::fs::read_dir(home.join(".on-n-off").join("limits"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .unwrap();
    let mut stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    stored["windows"][0]["usedPercent"] = serde_json::json!(140.0);
    stored["workspaceCredits"]["usedPercent"] = serde_json::json!(250.0);
    std::fs::write(&file, stored.to_string()).unwrap();
    let failed = ProviderLimitsDto {
        status: LimitsStatus::Failed,
        message: Some("Refresh failed.".into()),
        ..ProviderLimitsDto::for_test(AgentId::Codex, "acct-1")
    };

    let paused = crate::limits::remember(&home, failed).card;
    let reading = NotchProvider::current(vec![paused]).unwrap();
    let only = NotchSettings {
        providers: vec![AgentId::Codex],
        ..NotchSettings::default()
    };
    let sent = encoded(only, vec![(AgentId::Codex, Some(reading), Vec::new())])["cells"][0]
        ["reading"]
        .clone();

    let used: Vec<f64> = sent["windows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|window| window["usedPercent"].as_f64().unwrap())
        .collect();
    assert_eq!(used, [100.0]);
    assert_eq!(sent["workspaceCredits"]["usedPercent"], 100.0);
    let _ = std::fs::remove_dir_all(&home);
}
