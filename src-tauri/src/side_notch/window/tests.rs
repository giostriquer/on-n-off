use super::*;
use crate::side_notch::model::NotchSettings;
use crate::side_notch::sessions::SessionStatus;

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
fn sends_only_the_current_account_and_omits_account_identifiers() {
    let entries: Vec<ProviderLimitsDto> = serde_json::from_value(serde_json::json!([
        {"provider":"claude","status":"ok","currentAccount":false,"plan":"remembered-plan","windows":[]},
        {"provider":"claude","status":"signedOut","currentAccount":true,"account":{"id":"private-id","label":"private-label"},"plan":"max","windows":[]}
    ])).unwrap();
    let entry = current_provider(entries).unwrap();
    assert_eq!(entry.status, LimitsStatus::SignedOut);
    assert_eq!(entry.plan.as_deref(), Some("max"));
    let payload = serde_json::to_value(MessageProvider {
        entry: &entry,
        sessions: &[],
    })
    .unwrap();
    assert!(payload.get("account").is_none());
    assert!(payload.get("credits").is_none());
    assert_eq!(payload["provider"], "claude");
    assert_eq!(payload["sessions"], serde_json::json!([]));
    assert!(current_provider(Vec::new()).is_none());
}

#[test]
fn the_message_lists_selected_providers_in_rail_order_with_their_sessions() {
    let entry = |provider: AgentId| NativeProvider {
        provider,
        status: LimitsStatus::Ok,
        current_account: true,
        plan: None,
        message: None,
        windows: Vec::new(),
    };
    let providers = [
        Poll::new(Some(entry(AgentId::Claude))),
        Poll::new(Some(entry(AgentId::Codex))),
        Poll::new(None),
        Poll::new(Some(entry(AgentId::Cursor))),
    ];
    let session = LiveSession {
        id: "s".into(),
        name: "repo-1a".into(),
        place: "Terminal".into(),
        project: "repo".into(),
        status: SessionStatus::Working,
        last_active_at: "2026-09-01T10:00:00Z".into(),
    };
    let sessions = vec![vec![session.clone()], vec![], vec![], vec![]];
    let snapshot = NotchSnapshot {
        revision: 0,
        supported: true,
        settings: NotchSettings {
            providers: vec![AgentId::Cursor, AgentId::Claude, AgentId::Antigravity],
            ..NotchSettings::default()
        },
        displays: Vec::new(),
        error: None,
    };
    let listed = message_providers(&snapshot, &providers, &sessions);
    assert_eq!(
        listed
            .iter()
            .map(|entry| entry.entry.provider)
            .collect::<Vec<_>>(),
        [AgentId::Claude, AgentId::Cursor],
        "Codex is deselected and Antigravity has not been read yet"
    );
    assert_eq!(listed[0].sessions, [session]);
    assert!(listed[1].sessions.is_empty());
}

#[test]
fn pull_requests_keep_only_the_selected_lists_capped_with_row_fields() {
    let pr = |number: u64| {
        serde_json::json!({
            "id": format!("node-{number}"), "number": number, "title": format!("Fix #{number}"),
            "url": format!("https://github.com/octo/tools/pull/{number}"), "repo": "octo/tools",
            "author": "giovanne", "isDraft": false, "reviewDecision": "APPROVED", "ci": "success",
            "headRef": "fix", "baseRef": "main", "updatedAt": "2026-09-01T10:00:00Z",
            "mergeKind": "ready"
        })
    };
    let many: Vec<_> = (1..=30).map(pr).collect();
    let mut offsite = pr(7);
    offsite["url"] = serde_json::json!("http://example.com/pull/7");
    let dto: GithubPrsDto = serde_json::from_value(serde_json::json!({
        "status": "ok", "stale": false, "scope": ["org:octo"],
        "mine": {"total": 30, "items": many},
        "reviewRequested": {"total": 2, "items": [offsite, pr(99)]},
        "assigned": {"total": 0, "items": []}
    }))
    .unwrap();
    let review = native_pull_requests(&dto, &[GithubList::ReviewRequested]);
    assert_eq!(
        review.lists[0]
            .items
            .iter()
            .map(|pr| pr.number)
            .collect::<Vec<_>>(),
        [99],
        "rows that do not live on github.com never reach the helper"
    );
    let native = native_pull_requests(&dto, &[GithubList::Mine, GithubList::Assigned]);
    assert_eq!(native.lists.len(), 2, "review requests were not selected");
    assert_eq!(native.lists[0].id, GithubList::Mine);
    assert_eq!(native.lists[0].total, 30);
    assert_eq!(native.lists[0].items.len(), MAX_PULL_REQUESTS);
    assert_eq!(native.lists[1].items.len(), 0);
    let json = serde_json::to_value(&native).unwrap();
    assert_eq!(json["lists"][0]["id"], "mine");
    assert_eq!(json["lists"][1]["id"], "assigned");
    let row = &json["lists"][0]["items"][0];
    assert_eq!(row["title"], "Fix #1");
    assert_eq!(row["reviewDecision"], "APPROVED");
    assert_eq!(row["mergeKind"], "ready");
    assert_eq!(row["ci"], "success");
    assert!(
        row.get("headRef").is_none(),
        "branch names are not needed in the notch"
    );
}

#[test]
fn a_shared_cache_poll_is_due_as_soon_as_another_consumer_refreshes() {
    let now = Instant::now();
    let interval = Duration::from_secs(5 * 60);
    let mut poll = Poll::new(0u8);
    assert!(poll.due_from(now, interval, 0), "never read yet");
    poll.start(now);
    poll.finish_from(1, now, 7);

    let soon = now + Duration::from_secs(1);
    assert!(
        !poll.due_from(soon, interval, 7),
        "nothing new in the shared cache, so wait out the interval"
    );
    assert!(
        poll.due_from(soon, interval, 8),
        "the Limits screen's refresh replaced the cached read: pick it up now, not in five minutes"
    );
    poll.start(soon);
    assert!(!poll.due_from(soon, interval, 9), "one read in flight");
    poll.finish_from(2, soon, 9);
    assert!(!poll.due_from(soon, interval, 9));
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
