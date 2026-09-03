use super::*;

fn display(id: &str, x: f64, scale: f64) -> Display {
    Display {
        id: id.into(),
        name: "Same monitor name".into(),
        x,
        y: 0.0,
        width: 1728.0,
        height: 1117.0,
        work_y: 33.0,
        work_height: 1084.0,
        scale,
        mirrored: false,
    }
}

fn settings(id: &str, edge: Edge) -> NotchSettings {
    NotchSettings {
        enabled: true,
        display_id: Some(id.into()),
        edge,
        pull_requests: NotchPullRequests {
            enabled: false,
            lists: vec![GithubList::Mine],
        },
        ..NotchSettings::default()
    }
}

#[test]
fn follows_the_selected_uuid_across_reordering_and_mixed_scales() {
    let displays = vec![
        display("external", 0.0, 1.0),
        display("retina", -1728.0, 2.0),
    ];
    // Four cells: 4 × 73 + 3 × 8 + 2 × 40 = 396.
    assert_eq!(
        layout(&settings("retina", Edge::Right), &displays),
        Some(Layout {
            x: -76.0,
            y: 377.0,
            width: 76.0,
            height: 396.0
        })
    );
    let reversed: Vec<_> = displays.into_iter().rev().collect();
    assert_eq!(
        layout(&settings("retina", Edge::Left), &reversed),
        Some(Layout {
            x: -1728.0,
            y: 377.0,
            width: 76.0,
            height: 396.0
        })
    );
}

#[test]
fn never_falls_back_to_another_display_when_the_selection_is_missing_or_mirrored() {
    let mut current = settings("missing", Edge::Left);
    let mut displays = vec![display("other", 0.0, 1.0)];
    assert_eq!(layout(&current, &displays), None);
    current.display_id = Some("other".into());
    displays[0].mirrored = true;
    assert_eq!(layout(&current, &displays), None);
}

#[test]
fn top_and_bottom_edges_center_a_horizontal_rail_inside_the_work_area() {
    let displays = [display("main", 100.0, 2.0)];
    // Four cells side by side: 4 × 76 + 3 × 8 + 2 × 40 = 408, as tall as a cell.
    let top = layout(&settings("main", Edge::Top), &displays).unwrap();
    assert_eq!(
        top,
        Layout {
            x: 100.0 + (1728.0 - 408.0) / 2.0,
            y: 33.0,
            width: 408.0,
            height: 73.0
        }
    );
    let bottom = layout(&settings("main", Edge::Bottom), &displays).unwrap();
    assert_eq!(bottom.y, 33.0 + 1084.0 - 73.0);
    assert_eq!(bottom.x, top.x);
}

#[test]
fn the_rail_shrinks_with_fewer_providers_and_hides_without_any() {
    let displays = [display("main", 0.0, 1.0)];
    let mut current = settings("main", Edge::Right);
    current.providers = vec![AgentId::Cursor, AgentId::Claude, AgentId::Claude];
    assert_eq!(current.rail_providers(), [AgentId::Claude, AgentId::Cursor]);
    assert_eq!(layout(&current, &displays).unwrap().height, 234.0);
    current.providers.clear();
    assert_eq!(layout(&current, &displays), None);
}

#[test]
fn a_rail_that_does_not_fit_stays_hidden_instead_of_overflowing() {
    let mut small = display("small", 0.0, 1.0);
    small.work_height = 300.0;
    assert_eq!(
        layout(&settings("small", Edge::Right), &[small.clone()]),
        None
    );
    small.width = 300.0;
    small.work_height = 1000.0;
    assert_eq!(layout(&settings("small", Edge::Top), &[small]), None);
}

#[test]
fn the_pull_request_cell_is_on_by_default_with_only_the_users_own_list() {
    let defaults = NotchSettings::default();
    assert!(defaults.pull_requests.enabled);
    assert_eq!(defaults.pull_requests.lists, [GithubList::Mine]);
    assert_eq!(defaults.cell_count(), 5);
    let legacy: NotchSettings =
        serde_json::from_str(r#"{"enabled":true,"displayId":"main","edge":"right"}"#).unwrap();
    assert_eq!(legacy.cell_count(), 5, "older documents gain the cell");
    let mut current = settings("main", Edge::Right);
    current.pull_requests = NotchPullRequests {
        enabled: true,
        lists: vec![GithubList::Assigned, GithubList::Mine, GithubList::Assigned],
    };
    assert_eq!(
        current.pull_requests.selected_lists(),
        [GithubList::Mine, GithubList::Assigned]
    );
    let displays = [display("main", 0.0, 1.0)];
    assert_eq!(
        layout(&current, &displays).unwrap().height,
        rail_length(5, CELL_HEIGHT)
    );
    let json = serde_json::to_value(&current).unwrap();
    assert_eq!(json["pullRequests"]["enabled"], true);
    assert_eq!(json["pullRequests"]["lists"][0], "assigned");
}

#[test]
fn remains_hidden_until_explicitly_enabled() {
    assert_eq!(
        layout(&NotchSettings::default(), &[display("main", 0.0, 1.0)]),
        None
    );
}

#[test]
fn legacy_documents_default_to_always_show_all_providers_and_the_standard_size() {
    let legacy: NotchSettings =
        serde_json::from_str(r#"{"enabled":true,"displayId":"main","edge":"right"}"#).unwrap();
    assert_eq!(legacy.size, NotchSize::Standard);
    assert_eq!(legacy.show, ShowMode::Always);
    assert_eq!(legacy.providers, RAIL_ORDER.to_vec());
    let hover: NotchSettings = serde_json::from_str(
        r#"{"enabled":true,"displayId":"main","edge":"top","show":"onHover","providers":["codex"]}"#,
    )
    .unwrap();
    assert_eq!(hover.show, ShowMode::OnHover);
    assert_eq!(hover.edge, Edge::Top);
    assert_eq!(hover.providers, vec![AgentId::Codex]);
    let json = serde_json::to_value(&hover).unwrap();
    assert_eq!(json["show"], "onHover");
    assert_eq!(json["edge"], "top");
}

#[test]
fn presets_scale_the_whole_rail() {
    let displays = [display("main", 0.0, 1.0)];
    for (size, scale) in [
        (NotchSize::Compact, 0.875),
        (NotchSize::Standard, 1.0),
        (NotchSize::Large, 1.125),
    ] {
        let mut current = settings("main", Edge::Right);
        current.size = size;
        let rail = layout(&current, &displays).unwrap();
        assert_eq!(rail.width, CELL_WIDTH * scale);
        assert_eq!(rail.height, rail_length(4, CELL_HEIGHT) * scale);
    }
}
