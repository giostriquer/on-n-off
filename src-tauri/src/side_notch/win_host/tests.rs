use super::*;
use crate::dto::LimitWindowKind;
use crate::side_notch::model::NotchSettings;
use std::time::Duration;

fn snapshot() -> NotchSnapshot {
    NotchSnapshot {
        revision: 1,
        supported: true,
        settings: NotchSettings {
            enabled: true,
            ..NotchSettings::default()
        },
        displays: vec![],
        error: None,
    }
}

fn provider_entry(provider: AgentId, percent: f64) -> NativeProvider {
    NativeProvider {
        provider,
        status: LimitsStatus::Ok,
        message: None,
        windows: vec![LimitWindowDto {
            id: "w".into(),
            label: "Current session".into(),
            kind: LimitWindowKind::Session,
            used_percent: percent,
            resets_at: None,
            window_seconds: None,
            observed_at: "2026-09-01T10:00:00Z".into(),
        }],
    }
}

#[test]
fn outbound_delivery_is_dirty_driven_with_a_slow_heartbeat() {
    let now = Instant::now();
    let mut delivery = Delivery::new(now);
    assert!(delivery.due(now));
    delivery.sent(now);
    assert!(!delivery.due(now + Duration::from_secs(2)));
    delivery.mark_dirty();
    assert!(delivery.due(now + Duration::from_secs(2)));
    delivery.sent(now + Duration::from_secs(2));
    assert!(!delivery.due(now + Duration::from_secs(20)));
    assert!(delivery.due(now + HEARTBEAT_INTERVAL + Duration::from_secs(2)));
    assert!(HEARTBEAT_INTERVAL >= Duration::from_secs(30));
}

#[test]
fn a_poll_refreshes_on_its_interval_forces_once_and_never_stays_loading_forever() {
    let now = Instant::now();
    let interval = Duration::from_secs(5 * 60);
    let mut poll = Poll::new(0u8);
    assert!(poll.due(now, interval), "never read yet");
    assert!(!poll.start(now));
    assert!(!poll.due(now, interval), "one read in flight");
    poll.finish(1, now);
    assert!(!poll.due(now + Duration::from_secs(299), interval));
    assert!(poll.due(now + interval, interval));
    poll.force = true;
    assert!(poll.due(now + Duration::from_secs(1), interval));
    assert!(poll.start(now), "the forced read reports force once");
    assert!(!poll.force);
    poll.release_stale(now + READ_DEADLINE);
    assert!(poll.loading, "still inside the deadline");
    poll.release_stale(now + READ_DEADLINE + Duration::from_secs(1));
    assert!(
        !poll.loading,
        "a read that never reported back releases its slot"
    );
    assert_eq!(poll.value, 1);
}

#[test]
fn current_provider_drops_everything_but_the_signed_in_account() {
    let entries: Vec<ProviderLimitsDto> = serde_json::from_value(serde_json::json!([
        {"provider":"claude","status":"ok","currentAccount":false,"plan":"remembered-plan","windows":[]},
        {"provider":"claude","status":"signedOut","currentAccount":true,"account":{"id":"private-id","label":"private-label"},"windows":[]}
    ]))
    .unwrap();
    let entry = current_provider(entries).expect("the current account");
    assert_eq!(entry.status, LimitsStatus::SignedOut);
    assert!(current_provider(Vec::new()).is_none());
}

#[test]
fn rail_cells_list_selected_providers_in_rail_order_with_prs_last() {
    let providers: [Poll<Option<NativeProvider>>; PROVIDER_COUNT] =
        [(); PROVIDER_COUNT].map(|_| Poll::new(None));
    let mut with_data = providers;
    with_data[0] = Poll::new(Some(provider_entry(AgentId::Claude, 10.0)));
    with_data[3] = Poll::new(Some(provider_entry(AgentId::Cursor, 20.0)));
    let session_rows: Vec<Vec<LiveSession>> = vec![Vec::new(); PROVIDER_COUNT];
    let mut pulls = Poll::new(None);
    let mut snapshot = snapshot();
    snapshot.settings.providers = vec![AgentId::Cursor, AgentId::Claude, AgentId::Antigravity];
    let cells = rail_cells(&snapshot, &with_data, &session_rows, &pulls);
    assert_eq!(
        cells
            .iter()
            .map(|cell| match cell {
                CellData::Provider(provider) => Some(provider.provider),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [Some(AgentId::Claude), Some(AgentId::Cursor)],
        "Codex is deselected and Antigravity has not been read yet"
    );
    assert!(
        matches!(cells.last(), Some(CellData::Provider(_))),
        "the PR cell is off"
    );

    pulls.finish(Some(prs_dto()), Instant::now());
    let cells = rail_cells(&snapshot, &with_data, &session_rows, &pulls);
    assert!(
        matches!(cells.last(), Some(CellData::PullRequests(_))),
        "the PR cell closes the rail"
    );
}

fn prs_dto() -> GithubPrsDto {
    serde_json::from_value(serde_json::json!({
        "status": "ok", "stale": false, "scope": [],
        "mine": {"total": 2, "items": [
            {"id": "n1", "number": 1, "title": "Fix", "url": "https://github.com/octo/tools/pull/1",
             "repo": "octo/tools", "author": "g", "isDraft": false, "ci": "success",
             "headRef": "fix", "baseRef": "main", "updatedAt": "2026-09-01T10:00:00Z"},
            {"id": "n2", "number": 2, "title": "Offsite", "url": "http://example.com/pull/2",
             "repo": "octo/other", "author": "g", "isDraft": false, "ci": "none",
             "headRef": "fix", "baseRef": "main", "updatedAt": "2026-09-01T10:00:00Z"}
        ]},
        "reviewRequested": {"total": 0, "items": []},
        "assigned": {"total": 0, "items": []}
    }))
    .unwrap()
}

#[test]
fn the_pr_cell_keeps_only_the_selected_lists_capped_and_onsite() {
    let dto: GithubPrsDto = prs_dto();
    let cell = pr_cell(&dto, &[GithubList::Mine]);
    assert_eq!(cell.lists.len(), 1);
    assert_eq!(cell.lists[0].total, 2);
    assert_eq!(
        cell.lists[0].items.len(),
        1,
        "rows that are not on github.com never render"
    );
    assert_eq!(
        cell.lists[0].items[0].url,
        "https://github.com/octo/tools/pull/1"
    );
    assert!(
        cell.lists[0].items[0].review_decision.is_none(),
        "no review decision recorded on this row"
    );
}

#[test]
fn a_rail_data_frame_carries_settings_cells_and_errors() {
    let providers: [Poll<Option<NativeProvider>>; PROVIDER_COUNT] =
        [(); PROVIDER_COUNT].map(|_| Poll::new(None));
    let session_poll: Poll<Vec<Vec<LiveSession>>> = Poll::new(vec![Vec::new(); PROVIDER_COUNT]);
    let pulls: Poll<Option<GithubPrsDto>> = Poll::new(None);
    let frame = rail_data(&snapshot(), &providers, &session_poll, &pulls, Some("boom"));
    assert_eq!(frame.settings, snapshot().settings);
    assert_eq!(frame.action_error.as_deref(), Some("boom"));
    assert!(
        frame.cells.is_empty(),
        "nothing has been read yet, so nothing renders"
    );
}
