use super::*;
use crate::dto::{LimitWindowDto, LimitWindowKind, LimitsStatus};
use crate::side_notch::model::{Display, NotchSettings, ShowMode, RAIL_ORDER};

fn display(id: &str, x: f64, y: f64, width: f64, height: f64, scale: f64) -> Display {
    Display {
        id: id.into(),
        name: id.into(),
        x,
        y,
        width,
        height,
        work_y: y,
        work_height: height,
        scale,
        mirrored: false,
    }
}

fn settings() -> NotchSettings {
    NotchSettings {
        enabled: true,
        ..NotchSettings::default()
    }
}

#[allow(dead_code)]
fn data(cells: Vec<CellData>, action_error: Option<String>) -> RailData {
    RailData {
        settings: NotchSettings::default(),
        cells,
        action_error,
    }
}

/// Visual harness: renders realistic scenes to PNGs so layout bugs can be seen
/// without launching the app. Run with:
/// `cargo test --lib side_notch::win_paint::tests::visual_dump -- --ignored --no-capture`
#[ignore]
#[test]
fn visual_dump() {
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../.tmp-visual");
    let _ = std::fs::create_dir_all(out);

    let displays = vec![
        display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0),
        display("d2", 1920.0, 0.0, 1920.0, 1080.0, 1.0),
    ];

    let mut settings = settings();
    settings.display_id = Some("d1".into());
    settings.providers = RAIL_ORDER.to_vec();

    // Realistic Claude entry with the labels the live API produces.
    let claude = ProviderData {
        provider: AgentId::Claude,
        status: LimitsStatus::Ok,
        message: None,
        windows: vec![
            LimitWindowDto {
                id: "w1".into(),
                label: "5 hour · all models".into(),
                kind: LimitWindowKind::Session,
                used_percent: 72.0,
                resets_at: Some("2026-09-03T19:59:00Z".into()),
                window_seconds: None,
                observed_at: "2026-09-03T10:00:00Z".into(),
            },
            LimitWindowDto {
                id: "w2".into(),
                label: "Weekly · all models".into(),
                kind: LimitWindowKind::Weekly,
                used_percent: 27.0,
                resets_at: Some("2026-09-07T10:59:00Z".into()),
                window_seconds: None,
                observed_at: "2026-09-03T10:00:00Z".into(),
            },
            LimitWindowDto {
                id: "w3".into(),
                label: "Weekly · Fable".into(),
                kind: LimitWindowKind::Model,
                used_percent: 100.0,
                resets_at: Some("2026-09-07T10:59:00Z".into()),
                window_seconds: None,
                observed_at: "2026-09-03T10:00:00Z".into(),
            },
        ],
        sessions: vec![
            LiveSession {
                id: "s1".into(),
                name: "onosterm-56".into(),
                place: "Terminal".into(),
                project: "consolem".into(),
                status: crate::side_notch::sessions::SessionStatus::Working,
                last_active_at: "2026-09-03T10:19:00Z".into(),
            },
            LiveSession {
                id: "s2".into(),
                name: "web-app".into(),
                place: "VS Code".into(),
                project: "dashboard".into(),
                status: crate::side_notch::sessions::SessionStatus::Idle,
                last_active_at: "2026-09-01T10:19:00Z".into(),
            },
        ],
    };
    let codex = ProviderData {
        provider: AgentId::Codex,
        status: LimitsStatus::Ok,
        message: None,
        windows: vec![LimitWindowDto {
            id: "c1".into(),
            label: "Weekly · all models".into(),
            kind: LimitWindowKind::Weekly,
            used_percent: 47.0,
            resets_at: Some("2026-09-07T10:59:00Z".into()),
            window_seconds: None,
            observed_at: "2026-09-03T10:00:00Z".into(),
        }],
        sessions: Vec::new(),
    };
    let antigravity = ProviderData {
        provider: AgentId::Antigravity,
        status: LimitsStatus::Unsupported,
        message: Some("Antigravity has no subscription limits to show.".into()),
        windows: Vec::new(),
        sessions: Vec::new(),
    };
    let cursor = ProviderData {
        provider: AgentId::Cursor,
        status: LimitsStatus::Ok,
        message: None,
        windows: Vec::new(),
        sessions: Vec::new(),
    };
    let pr_cell = PrCellData {
        status: GithubStatus::Ok,
        hint: None,
        stale: false,
        lists: vec![PrListData {
            id: GithubList::Mine,
            total: 12,
            items: (1..=6)
                .map(|n| PrRowData {
                    id: format!("n{n}"),
                    number: n,
                    title: format!("Fix the flaky resize test number {n} on Windows builds"),
                    url: format!("https://github.com/octo/tools/pull/{n}"),
                    repo: "octo/tools".into(),
                    is_draft: n == 2,
                    review_decision: if n % 2 == 0 {
                        Some(ReviewDecision::Approved)
                    } else {
                        Some(ReviewDecision::ChangesRequested)
                    },
                    ci: if n % 3 == 0 {
                        CiState::Failure
                    } else {
                        CiState::Success
                    },
                    merge_kind: if n == 1 { Some(MergeKind::Ready) } else { None },
                })
                .collect(),
        }],
    };

    let data = RailData {
        settings: settings.clone(),
        cells: vec![
            CellData::Provider(claude),
            CellData::Provider(codex),
            CellData::Provider(antigravity),
            CellData::Provider(cursor),
            CellData::PullRequests(pr_cell),
        ],
        action_error: None,
    };

    // Scale 2.0 variant: the user runs >100% display scaling.
    let scale2 = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 2.0)];
    let mut s2 = settings.clone();
    s2.display_id = Some("d1".into());
    let d2 = RailData {
        settings: s2.clone(),
        cells: data.cells.clone(),
        action_error: None,
    };
    let hover2 = Hover {
        active: Some(0),
        ..Hover::default()
    };
    let planned = plan(&s2, &scale2, &d2, hover2).expect("scale2 popover");
    std::fs::write(
        format!("{out}/scale2-popover.png"),
        render(&planned).encode_png().unwrap(),
    )
    .unwrap();

    // One popover dump per rail cell, plus the plain rail and the collapsed pill.
    for cell in 0..data.cells.len() {
        let hover = Hover {
            active: Some(cell),
            ..Hover::default()
        };
        let Some(planned) = plan(&settings, &displays, &data, hover) else {
            panic!("cell {cell} should plan");
        };
        let pix = render(&planned);
        std::fs::write(
            format!("{out}/popover-cell{cell}.png"),
            pix.encode_png().unwrap(),
        )
        .unwrap();
        let _ = cell;
    }

    let planned = plan(&settings, &displays, &data, Hover::default()).expect("rail");
    std::fs::write(
        format!("{out}/rail.png"),
        render(&planned).encode_png().unwrap(),
    )
    .unwrap();

    let mut hover_settings = settings.clone();
    hover_settings.show = ShowMode::OnHover;
    let planned = plan(&hover_settings, &displays, &data, Hover::default()).expect("pill");
    std::fs::write(
        format!("{out}/pill.png"),
        render(&planned).encode_png().unwrap(),
    )
    .unwrap();

    // Top edge variant with the popover open.
    let mut top = settings.clone();
    top.edge = Edge::Top;
    let top_data = RailData {
        settings: top.clone(),
        cells: data.cells.clone(),
        action_error: Some("Example action error text.".into()),
    };
    let planned = plan(
        &top,
        &displays,
        &top_data,
        Hover {
            active: Some(0),
            ..Hover::default()
        },
    )
    .expect("top rail");
    std::fs::write(
        format!("{out}/top-popover.png"),
        render(&planned).encode_png().unwrap(),
    )
    .unwrap();
}
