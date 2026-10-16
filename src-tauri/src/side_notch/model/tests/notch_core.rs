use super::*;

fn rail(id: &str, edge: Edge, size: NotchSize) -> NotchSettings {
    NotchSettings {
        size,
        ..settings(id, edge)
    }
}

fn contains(outer: &Layout, inner: &Layout) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

fn frame(x: f64, y: f64, width: f64, height: f64) -> Option<Layout> {
    Some(Layout {
        x,
        y,
        width,
        height,
    })
}

#[test]
fn rail_frames_follow_the_selected_uuid_on_every_edge() {
    let displays = [
        display("external", 0.0, 2.0),
        display("retina", -1728.0, 2.0),
    ];
    let reversed = [displays[1].clone(), displays[0].clone()];
    let standard = NotchSize::Standard;
    assert_eq!(
        layout(&rail("retina", Edge::Right, standard), &displays),
        frame(-76.0, 336.5, 76.0, 477.0)
    );
    assert_eq!(
        layout(&rail("retina", Edge::Left, standard), &reversed),
        frame(-1728.0, 336.5, 76.0, 477.0)
    );
    assert_eq!(
        layout(&rail("external", Edge::Top, standard), &displays),
        frame(618.0, 33.0, 492.0, 73.0)
    );
    assert_eq!(
        layout(&rail("external", Edge::Bottom, standard), &displays),
        frame(618.0, 33.0 + 1084.0 - 73.0, 492.0, 73.0)
    );
}

#[test]
fn the_rail_shrinks_with_fewer_providers_and_hides_without_any_or_without_room() {
    let displays = [display("main", 0.0, 2.0)];
    let mut two = settings("main", Edge::Right);
    two.providers = vec![AgentId::Cursor, AgentId::Claude, AgentId::Claude];
    assert_eq!(two.rail_providers(), [AgentId::Claude, AgentId::Cursor]);
    assert_eq!(layout(&two, &displays).map(|rail| rail.height), Some(234.0));
    let mut none = settings("main", Edge::Right);
    none.providers.clear();
    assert_eq!(layout(&none, &displays), None);
    none.pull_requests.enabled = true;
    assert_eq!(
        layout(&none, &displays).map(|rail| rail.height),
        Some(73.0 + 80.0)
    );
    let short = Display {
        id: "short".into(),
        name: "LG".into(),
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 300.0,
        work_y: 30.0,
        work_height: 270.0,
        scale: 1.0,
        mirrored: false,
    };
    assert_eq!(layout(&settings("short", Edge::Right), &[short]), None);
}

#[test]
fn missing_mirrored_ambiguous_and_disabled_displays_hide_without_fallback() {
    let selected = settings("chosen", Edge::Right);
    let mut mirrored = display("chosen", 0.0, 2.0);
    mirrored.mirrored = true;
    for displays in [
        vec![display("other", 0.0, 2.0)],
        vec![mirrored],
        vec![display("chosen", 0.0, 2.0), display("chosen", 0.0, 2.0)],
    ] {
        assert_eq!(layout(&selected, &displays), None);
        assert_eq!(pill_frame(&selected, &displays), None);
    }
    assert_eq!(
        layout(&NotchSettings::default(), &[display("chosen", 0.0, 2.0)]),
        None
    );
}

#[test]
fn presets_scale_the_whole_rail_on_the_pixel_grid() {
    let displays = [display("main", 0.0, 1.0)];
    for (size, scale) in [
        (NotchSize::Compact, 0.875_f64),
        (NotchSize::Standard, 1.0),
        (NotchSize::Large, 1.125),
    ] {
        let frame = layout(&rail("main", Edge::Right, size), &displays).expect("fits");
        assert_eq!(frame.width, (76.0 * scale).round(), "{size:?}");
        assert_eq!(
            frame.height,
            rail_metrics(size, 1.0, Edge::Right).length(5),
            "{size:?}"
        );
        for value in [frame.x, frame.y, frame.width, frame.height] {
            assert_eq!(value, value.round(), "{size:?}: {frame:?}");
        }
    }
}

#[test]
fn pills_hug_the_edge_centred_on_the_rail() {
    let displays = [display("main", 0.0, 1.0)];
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        let current = rail("main", edge, NotchSize::Standard);
        let rail = layout(&current, &displays).expect("fits");
        let pill = pill_frame(&current, &displays).expect("fits");
        assert!(contains(&rail, &pill), "{edge:?}: {pill:?} in {rail:?}");
        let (flush, edge_of_rail) = match edge {
            Edge::Left => (pill.x, rail.x),
            Edge::Right => (pill.x + pill.width, rail.x + rail.width),
            Edge::Top => (pill.y, rail.y),
            Edge::Bottom => (pill.y + pill.height, rail.y + rail.height),
        };
        assert_eq!(flush, edge_of_rail, "{edge:?}");
        let (pill_middle, rail_middle) = if edge.is_vertical() {
            (pill.y + pill.height / 2.0, rail.y + rail.height / 2.0)
        } else {
            (pill.x + pill.width / 2.0, rail.x + rail.width / 2.0)
        };
        assert_eq!(pill_middle, rail_middle, "{edge:?}");
    }
}

#[test]
fn cells_tile_the_rail() {
    let metrics = rail_metrics(NotchSize::Standard, 1.0, Edge::Right);
    let cells = rail_cell_frames(Edge::Right, &metrics, 4);
    assert_eq!(cells.len(), 4);
    assert_eq!(cells[0].y, metrics.ear());
    assert_eq!(
        cells[3].y + cells[3].height,
        metrics.length(4) - metrics.ear()
    );
    let bar = rail_metrics(NotchSize::Standard, 1.0, Edge::Top);
    assert_eq!(bar.thickness, 73.0);
    assert_eq!(rail_cell_frames(Edge::Top, &bar, 2)[1].x, 40.0 + 76.0 + 8.0);
    assert_eq!(
        cells[0].y,
        metrics.length(4) - (cells[3].y + cells[3].height)
    );
}
