use super::*;
use crate::side_notch::model::{Edge, GithubList, NotchPullRequests, NotchSize, NotchStyle};
use crate::side_notch::protocol::PROTOCOL_VERSION;

fn pinned_snapshot() -> NotchSnapshot {
    NotchSnapshot {
        revision: 0,
        supported: true,
        liquid_glass: true,
        settings: NotchSettings {
            enabled: true,
            display_id: Some("display-a".into()),
            edge: Edge::Left,
            size: NotchSize::Large,
            show: ShowMode::OnHover,
            providers: vec![AgentId::Muse, AgentId::Codex, AgentId::Claude],
            pull_requests: NotchPullRequests {
                enabled: true,
                lists: vec![GithubList::ReviewRequested, GithubList::Mine],
            },
            popover_style: NotchStyle::Solid,
        },
        displays: vec![Display {
            id: "display-a".into(),
            name: "Built-in Display".into(),
            x: 0.0,
            y: 0.0,
            width: 1512.0,
            height: 982.0,
            work_y: 33.0,
            work_height: 949.0,
            scale: 2.0,
            mirrored: false,
        }],
        error: None,
    }
}

fn pinned_limits(agent: AgentId) -> Vec<ProviderLimitsDto> {
    let cards = match agent {
        AgentId::Claude => serde_json::json!([
            {"provider":"claude","status":"ok","currentAccount":false,"plan":"pro","windows":[
                {"id":"weekly_all","label":"Weekly · all models","kind":"weekly","usedPercent":99.0,"observedAt":"2026-09-01T09:00:00Z"}
            ]},
            {"provider":"claude","status":"ok","currentAccount":true,
             "account":{"id":"acct-claude","label":"you@example.com"},"plan":"max","windows":[
                {"id":"weekly_all","label":"Weekly · all models","kind":"weekly","usedPercent":41.5,
                 "resetsAt":"2026-09-05T08:00:00Z","windowSeconds":604800,"observedAt":"2026-09-01T10:00:00Z"},
                {"id":"session","label":"5 hour · all models","kind":"session","usedPercent":12.0,
                 "resetsAt":"2026-09-01T13:00:00Z","observedAt":"2026-09-01T10:00:00Z"},
                {"id":"weekly_scoped:Fable","label":"Weekly · Fable","kind":"model","usedPercent":63.0,
                 "observedAt":"2026-09-01T10:00:00Z"}
            ]}
        ]),
        AgentId::Codex => serde_json::json!([
            {"provider":"codex","status":"failed","currentAccount":true,"message":"Codex did not answer.",
             "account":{"id":"acct-codex","label":"you@example.com"},"windows":[
                {"id":"weekly","label":"Weekly","kind":"weekly","usedPercent":22.0,
                 "resetsAt":"2026-09-04T00:00:00Z","observedAt":"2026-09-01T09:30:00Z"}
            ],
             "credits":{"balance":"12.50","unlimited":false},
             "workspaceCredits":{"limit":"25000","used":"8000.5","usedPercent":32.0,
                 "resetsAt":"2026-10-01T12:00:00+00:00","reached":false}}
        ]),
        AgentId::Muse => serde_json::json!([
            {"provider":"muse","status":"ok","currentAccount":false,"windows":[]}
        ]),
        _ => serde_json::json!([]),
    };
    serde_json::from_value(cards).unwrap()
}

fn pinned_sessions(agent: AgentId) -> Vec<LiveSession> {
    let session = |id: &str, name: &str, status: SessionStatus| LiveSession {
        id: id.into(),
        name: name.into(),
        place: "Terminal".into(),
        project: "webapp".into(),
        status,
        last_active_at: "2026-09-01T09:58:00Z".into(),
    };
    match agent {
        AgentId::Claude => vec![
            session("claude-1", "webapp-1a", SessionStatus::Working),
            session("claude-2", "webapp-2b", SessionStatus::Idle),
        ],
        AgentId::Muse => vec![session("muse-1", "webapp-mu", SessionStatus::Working)],
        _ => Vec::new(),
    }
}

fn pinned_pull_requests() -> GithubPrsDto {
    let pr = |number: u64, repo: &str| {
        serde_json::json!({
            "id": format!("node-{number}"), "number": number, "title": format!("Fix #{number}"),
            "url": format!("https://github.com/{repo}/pull/{number}"), "repo": repo,
            "author": "octocat", "isDraft": false, "ci": "success", "headRef": "fix",
            "baseRef": "main", "updatedAt": "2026-09-01T10:00:00Z"
        })
    };
    let mut work = pr(1, "acme/webapp");
    work["reviewDecision"] = "APPROVED".into();
    work["mergeKind"] = "ready".into();
    work["inOrganization"] = true.into();
    let mut personal = pr(2, "octocat/dotfiles");
    personal["isDraft"] = true.into();
    personal["ci"] = "failure".into();
    personal["reviewDecision"] = "CHANGES_REQUESTED".into();
    personal["inOrganization"] = false.into();
    let mut offsite = pr(3, "acme/webapp");
    offsite["url"] = "http://example.com/pull/3".into();
    let mut unread = pr(4, "acme/api");
    unread["mergeKind"] = "conflicts".into();
    serde_json::from_value(serde_json::json!({
        "status": "rateLimited", "hint": "Showing the pull requests read at 10:00.", "stale": true,
        "scope": ["org:acme"],
        "mine": {"total": 3, "items": [work, personal, offsite]},
        "reviewRequested": {"total": 2, "items": [unread, work.clone()]},
        "assigned": {"total": 1, "items": [pr(5, "acme/webapp")]}
    }))
    .unwrap()
}

const PINNED_ACTION_ERROR: &str = "Cannot open the Limits window.";

const GOLDEN: &str =
    include_str!("../../../../macos/SideNotch/Tests/NotchCoreTests/host-message.json");

fn sent() -> Vec<String> {
    let app = FakeApp::default();
    for agent in RAIL_ORDER {
        app.world().limits.insert(agent, pinned_limits(agent));
        app.world().sessions.insert(agent, pinned_sessions(agent));
    }
    app.world().pull_requests = Some(pinned_pull_requests());
    app.world().open_error = Some(PINNED_ACTION_ERROR.into());
    app.world().hold = true;
    let mut rig = Rig::new(app.clone(), pinned_snapshot());
    rig.tick(Duration::ZERO);
    app.world().hold = false;
    app.release();
    rig.act(Inbound::Action(Action::OpenLimits));
    let wire = rig.wire.wire();
    let first = (&wire.snapshots[0], &wire.frames[0]);
    let last = (
        wire.snapshots.last().expect("a frame was sent"),
        wire.frames.last().expect("a frame was sent"),
    );
    [first, last]
        .into_iter()
        .zip(1..)
        .map(|((snapshot, frame), sequence)| {
            String::from_utf8(
                crate::side_notch::protocol::encode(sequence, snapshot, frame).unwrap(),
            )
            .unwrap()
        })
        .collect()
}

#[test]
fn the_helper_receives_exactly_the_golden_protocol_7_messages() {
    assert_eq!(PROTOCOL_VERSION, 7);
    let golden: Vec<serde_json::Value> = serde_json::from_str(GOLDEN).expect("the golden messages");
    assert_eq!(
        sent(),
        golden
            .iter()
            .map(|message| serde_json::to_string(message).unwrap())
            .collect::<Vec<_>>(),
        "the encoder's bytes, key for key, are the first frame and a frame after every read, \
         the golden messages NotchCoreChecks decodes"
    );
    assert_eq!(
        serde_json::to_string_pretty(&golden).unwrap() + "\n",
        GOLDEN,
        "the golden file stays pretty-printed"
    );
}
