use super::*;
use crate::dto::AgentId;
use crate::dto::{LimitWindowDto, LimitWindowKind, Reading};
use crate::side_notch::model::frame;
use crate::side_notch::model::{Display, GithubList, NotchProvider, NotchSettings};
use crate::side_notch::plan::{CellId, FixedAdvance, R};

const CLAUDE: Option<CellId> = Some(CellId::Provider(AgentId::Claude));

fn display(id: &str, mirrored: bool) -> Display {
    Display {
        id: id.into(),
        name: id.into(),
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
        work_y: 0.0,
        work_height: 1040.0,
        scale: 1.0,
        mirrored,
    }
}

fn settings(show: ShowMode) -> NotchSettings {
    NotchSettings {
        enabled: true,
        display_id: Some("d1".into()),
        show,
        ..NotchSettings::default()
    }
}

pub fn data(show: ShowMode) -> RailData {
    let claude = NotchProvider::current(vec![crate::limits::signed_in_card(
        AgentId::Claude,
        "acct",
        Reading {
            windows: vec![LimitWindowDto {
                id: "w".into(),
                label: "Current session".into(),
                kind: LimitWindowKind::Session,
                used_percent: 10.0,
                resets_at: None,
                window_seconds: None,
                observed_at: "2026-09-01T10:00:00Z".into(),
                role: None,
                starts_on_first_message: false,
            }],
            ..Reading::default()
        },
    )])
    .expect("a signed-in account");
    frame::rail(
        &settings(show),
        [(AgentId::Claude, Some(claude), Vec::new())],
        None,
        None,
    )
}

fn clock() -> chrono::DateTime<chrono::Utc> {
    "2026-09-03T10:30:00Z".parse().unwrap()
}

fn cell_rect(presenter: &mut Presenter) -> R {
    presenter.present(clock()).expect("plan").cells[0].rect
}

#[test]
fn the_rail_is_hidden_until_enabled_data_and_displays_agree() {
    let mut presenter = Presenter::new(&FixedAdvance);
    assert!(presenter.present(clock()).is_none());
    presenter.accept(data(ShowMode::Always));
    assert!(presenter.present(clock()).is_none(), "no displays yet");
    presenter.set_displays(vec![display("d1", false)]);
    assert!(presenter.present(clock()).is_some());
}

#[test]
fn the_pointer_scale_is_the_drawn_rails_and_1x_while_a_mirrored_or_duplicated_selection_hides_it() {
    let mut presenter = Presenter::new(&FixedAdvance);
    assert_eq!(presenter.scale(), 1.0, "no settings yet");
    presenter.accept(data(ShowMode::Always));
    let mut scaled = display("d1", false);
    scaled.scale = 1.5;
    presenter.set_displays(vec![scaled.clone()]);
    assert_eq!(presenter.scale(), 1.0, "nothing drawn yet");
    presenter.present(clock());
    assert_eq!(presenter.scale(), 1.5);
    let mut mirrored = scaled.clone();
    mirrored.mirrored = true;
    presenter.set_displays(vec![mirrored]);
    assert!(presenter.present(clock()).is_none());
    assert_eq!(presenter.scale(), 1.0, "a mirrored selection");
    presenter.set_displays(vec![scaled.clone(), scaled]);
    assert!(presenter.present(clock()).is_none());
    assert_eq!(presenter.scale(), 1.0, "two displays with the selected id");
}

#[test]
fn the_hover_strip_opens_the_rail_and_the_grace_closes_it() {
    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.accept(data(ShowMode::OnHover));
    presenter.set_displays(vec![display("d1", false)]);
    assert!(
        presenter.present(clock()).expect("plan").pill.is_some(),
        "collapsed first"
    );

    let pill = presenter.present(clock()).unwrap().pill.unwrap();
    presenter.cursor_at(pill.mid_x(), pill.mid_y(), true, Instant::now());
    assert!(presenter.hover.rail_open);

    let cell = cell_rect(&mut presenter);
    let entered = Instant::now();
    presenter.cursor_at(cell.mid_x(), cell.mid_y(), true, entered);
    assert!(presenter.hover.active.is_none(), "not before the delay");
    presenter.advance(entered + HOVER_OPEN_DELAY - Duration::from_millis(1));
    assert!(
        presenter.hover.active.is_none(),
        "not a millisecond before it either"
    );
    presenter.advance(entered + HOVER_OPEN_DELAY + Duration::from_millis(5));
    assert_eq!(presenter.hover.active, CLAUDE);

    presenter.cursor_left();
    presenter.advance(entered + Duration::from_millis(200) + HOVER_CLOSE_GRACE);
    assert_eq!(presenter.hover.active, None);
    assert!(!presenter.hover.rail_open);
}

#[test]
fn a_click_pins_the_popover_until_the_same_cell_is_clicked_again() {
    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.accept(data(ShowMode::Always));
    presenter.set_displays(vec![display("d1", false)]);
    let now = Instant::now();

    let cell = cell_rect(&mut presenter);
    assert!(presenter
        .clicked(cell.mid_x() + 1000.0, cell.mid_y())
        .is_empty());
    assert_eq!(presenter.pinned(), None);

    let cell = cell_rect(&mut presenter);
    presenter.clicked(cell.mid_x(), cell.mid_y());
    assert_eq!(presenter.pinned(), CLAUDE);
    assert_eq!(presenter.hover.active, CLAUDE);

    presenter.cursor_left();
    presenter.advance(now + HOVER_CLOSE_GRACE);
    assert_eq!(
        presenter.hover.active, CLAUDE,
        "a fresh pin is not dropped by the grace"
    );

    let cell = cell_rect(&mut presenter);
    presenter.clicked(cell.mid_x(), cell.mid_y());
    assert_eq!(presenter.pinned(), None);
    assert_eq!(presenter.hover.active, None);
}

#[test]
fn a_click_outside_the_overlay_dismisses_a_pinned_popover() {
    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.accept(data(ShowMode::Always));
    presenter.set_displays(vec![display("d1", false)]);

    let cell = cell_rect(&mut presenter);
    presenter.clicked(cell.mid_x(), cell.mid_y());
    assert_eq!(presenter.pinned(), CLAUDE);
    presenter.take_dirty();

    presenter.dismiss();
    assert_eq!(presenter.pinned(), None);
    assert_eq!(presenter.hover.active, None);
    assert!(presenter.take_dirty(), "the dismissal asks for a repaint");

    presenter.dismiss();
    assert!(!presenter.take_dirty(), "an idle dismissal is not a change");
}

#[test]
fn clicking_the_cap_asks_for_the_show_mode_to_flip() {
    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.accept(data(ShowMode::Always));
    presenter.set_displays(vec![display("d1", false)]);
    let cap = presenter.present(clock()).unwrap().cap.rect;
    assert_eq!(
        presenter.clicked(cap.mid_x(), cap.mid_y()),
        vec![WinAction::SetShow(ShowMode::OnHover)]
    );

    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.accept(data(ShowMode::OnHover));
    presenter.set_displays(vec![display("d1", false)]);
    let pill = presenter.present(clock()).unwrap().pill.unwrap();
    presenter.cursor_at(pill.mid_x(), pill.mid_y(), true, Instant::now());
    let cap = presenter.present(clock()).unwrap().cap.rect;
    assert_eq!(
        presenter.clicked(cap.mid_x(), cap.mid_y()),
        vec![WinAction::SetShow(ShowMode::Always)]
    );
}

#[test]
fn the_deadline_never_sleeps_past_the_scheduled_open() {
    let entered = Instant::now();
    let idle = Presenter::new(&FixedAdvance);
    assert!(idle.deadline(entered) >= entered + Duration::from_millis(100));

    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.accept(data(ShowMode::Always));
    presenter.set_displays(vec![display("d1", false)]);
    let cell = cell_rect(&mut presenter);
    presenter.cursor_at(cell.mid_x(), cell.mid_y(), true, entered);
    let deadline = presenter.deadline(entered);
    assert!(
        deadline <= entered + HOVER_OPEN_DELAY,
        "the loop must wake for the hover-open delay"
    );
}

#[test]
fn a_layout_changing_settings_update_drops_hover_state() {
    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.accept(data(ShowMode::Always));
    presenter.set_displays(vec![display("d1", false)]);
    let cell = cell_rect(&mut presenter);
    let entered = Instant::now();
    presenter.cursor_at(cell.mid_x(), cell.mid_y(), true, entered);
    presenter.advance(entered + HOVER_OPEN_DELAY);
    presenter.clicked(cell.mid_x(), cell.mid_y());
    assert_eq!(presenter.pinned(), CLAUDE);

    let mut next = data(ShowMode::Always);
    next.settings.edge = crate::side_notch::model::Edge::Left;
    presenter.accept(next);
    presenter.present(clock());
    assert_eq!(presenter.pinned(), None, "panels moved; hover state resets");
    assert_eq!(presenter.hover, Hover::default());

    let moved = cell_rect(&mut presenter);
    let stayed = entered + Duration::from_millis(500);
    presenter.cursor_at(moved.mid_x(), moved.mid_y(), true, stayed);
    presenter.advance(stayed + HOVER_OPEN_DELAY);
    assert_eq!(
        presenter.hover.active, CLAUDE,
        "a pointer on the moved cell opens its popover after the delay"
    );
}

#[test]
fn a_collapsed_rail_answers_no_click_on_its_cells_or_its_cap() {
    let mut presenter = shown(data(ShowMode::OnHover));
    let plan = presenter.present(clock()).expect("plan");
    assert!(plan.pill.is_some(), "collapsed");
    let (cell, cap) = (plan.cells[0].rect, plan.cap.rect);
    for target in [cell, cap] {
        assert!(presenter.clicked(target.mid_x(), target.mid_y()).is_empty());
        assert_eq!(presenter.pinned(), None);
        assert_eq!(presenter.hover.active, None);
    }
}

#[test]
fn fullscreen_suppression_hides_the_whole_window() {
    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.accept(data(ShowMode::Always));
    presenter.set_displays(vec![display("d1", false)]);
    assert!(presenter.present(clock()).is_some());
    presenter.set_suppressed(true);
    assert!(
        presenter.present(clock()).is_none(),
        "a fullscreen app owns the screen"
    );
    presenter.set_suppressed(false);
    assert!(presenter.present(clock()).is_some());
}

#[test]
fn mirrored_or_missing_displays_hide_the_rail() {
    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.accept(data(ShowMode::Always));
    presenter.set_displays(vec![display("d1", true)]);
    assert!(presenter.present(clock()).is_none());
    presenter.set_displays(vec![display("other", false)]);
    assert!(presenter.present(clock()).is_none());
    presenter.set_displays(vec![display("d1", false)]);
    assert!(presenter.present(clock()).is_some());
}

#[test]
fn leaving_the_window_clears_the_cap_highlight() {
    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.set_displays(vec![display("d1", false)]);
    presenter.accept(data(ShowMode::Always));
    let cap = presenter.present(clock()).expect("plan").cap.rect;
    let now = Instant::now();
    presenter.cursor_at(cap.mid_x(), cap.mid_y(), true, now);
    assert!(
        presenter.hover.cap_hovered,
        "the pin lights under the pointer"
    );
    presenter.cursor_at(-40.0, -40.0, false, now + Duration::from_millis(100));
    assert!(
        !presenter.hover.cap_hovered,
        "and fades again once the pointer is gone"
    );
}

#[test]
fn the_collapsed_strip_still_gets_pointer_samples() {
    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.set_displays(vec![display("d1", false)]);
    presenter.accept(data(ShowMode::OnHover));
    assert!(
        presenter.present(clock()).expect("plan").pill.is_some(),
        "collapsed"
    );
    assert!(
        presenter.pointer_poll_active(),
        "the strip is watched while the rail is closed"
    );
    let now = Instant::now();
    assert!(
        presenter.deadline(now) <= now + POINTER_POLL,
        "and the loop wakes on the short cadence to do it"
    );
}

#[test]
fn the_screens_change_only_after_the_first_read() {
    let mut presenter = Presenter::new(&FixedAdvance);
    let changed = Some(WinAction::ScreensChanged);
    assert_eq!(
        presenter.set_displays(vec![display("d1", false)]),
        None,
        "the first read is what the host started with"
    );
    assert_eq!(presenter.set_displays(vec![display("d1", false)]), None);
    assert_eq!(
        presenter.set_displays(vec![display("d1", false), display("d2", false)]),
        changed
    );
    assert_eq!(
        presenter.set_displays(Vec::new()),
        changed,
        "a read that failed"
    );
    assert_eq!(presenter.set_displays(vec![display("d1", true)]), changed);
}

#[test]
fn window_input_reaches_the_presenter_in_points_of_the_selected_display() {
    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.accept(data(ShowMode::Always));
    let mut scaled = display("d1", false);
    scaled.scale = 2.0;
    presenter.set_displays(vec![scaled]);
    let device = |points: f64| (points * 2.0).round() as i32;

    let cap = presenter.present(clock()).unwrap().cap.rect;
    assert_eq!(
        presenter.input(
            Input::Pressed {
                x: device(cap.mid_x()),
                y: device(cap.mid_y()),
            },
            Instant::now(),
        ),
        vec![WinAction::SetShow(ShowMode::OnHover)]
    );

    let cell = cell_rect(&mut presenter);
    let entered = Instant::now();
    let moved = Input::Moved {
        x: device(cell.mid_x()),
        y: device(cell.mid_y()),
    };
    assert!(presenter.input(moved, entered).is_empty());
    presenter.advance(entered + HOVER_OPEN_DELAY);
    assert_eq!(presenter.hover.active, CLAUDE);

    assert!(presenter.input(Input::Left, entered).is_empty());
    assert!(!presenter.cursor_inside);

    let cell = cell_rect(&mut presenter);
    let pressed = Input::Pressed {
        x: device(cell.mid_x()),
        y: device(cell.mid_y()),
    };
    assert!(presenter.input(pressed, entered).is_empty());
    assert_eq!(presenter.pinned(), CLAUDE);
}

fn read_pulls(show: ShowMode) -> RailData {
    pull_request(show, 7, "Fix the resize test")
}

fn pull_request(show: ShowMode, number: u64, title: &str) -> RailData {
    let pulls: crate::dto::GithubPrsDto = serde_json::from_value(serde_json::json!({
        "status": "ok", "hint": null, "stale": false, "scope": [],
        "mine": {"total": 1, "items": [{
            "id": format!("n{number}"), "number": number, "title": title,
            "url": format!("https://github.com/acme/webapp/pull/{number}"), "repo": "acme/webapp",
            "author": "octocat", "isDraft": false, "ci": "success", "headRef": "fix",
            "baseRef": "main", "updatedAt": "2026-09-01T10:00:00Z"
        }]},
        "reviewRequested": {"total": 0, "items": []},
        "assigned": {"total": 0, "items": []}
    }))
    .expect("a pull request read");
    frame::rail(&settings(show), Vec::new(), Some(&pulls), None)
}

fn shown(data: RailData) -> Presenter {
    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.accept(data);
    presenter.set_displays(vec![display("d1", false)]);
    presenter
}

fn click_cell(presenter: &mut Presenter, index: usize) -> Vec<WinAction> {
    let cell = presenter.present(clock()).expect("plan").cells[index].rect;
    presenter.clicked(cell.mid_x(), cell.mid_y())
}

fn zone(presenter: &mut Presenter, wanted: fn(&Zone) -> bool) -> R {
    presenter
        .present(clock())
        .expect("plan")
        .popover
        .as_ref()
        .expect("the popover is open")
        .zones
        .iter()
        .find_map(|(zone, rect)| wanted(zone).then_some(*rect))
        .expect("the zone is planned")
}

#[test]
fn a_footer_click_opens_the_page_its_popover_belongs_to_and_refreshes() {
    let mut presenter = shown(read_pulls(ShowMode::Always));
    let last = presenter.data.as_ref().unwrap().cells.len() - 1;
    assert!(click_cell(&mut presenter, last).is_empty());
    let footer = zone(&mut presenter, |zone| matches!(zone, Zone::Footer(_)));
    assert_eq!(
        presenter.clicked(footer.mid_x(), footer.mid_y()),
        vec![WinAction::OpenPullRequests, WinAction::Refresh]
    );
    assert_eq!(presenter.pinned(), None, "the footer closes the popover");
    assert_eq!(presenter.hover.active, None);

    assert!(click_cell(&mut presenter, 0).is_empty());
    let footer = zone(&mut presenter, |zone| matches!(zone, Zone::Footer(_)));
    assert_eq!(
        presenter.clicked(footer.mid_x(), footer.mid_y()),
        vec![WinAction::OpenLimits, WinAction::Refresh]
    );
}

#[test]
fn a_row_click_opens_its_pull_request_and_its_copy_icon_copies_the_review_request() {
    let mut presenter = shown(read_pulls(ShowMode::Always));
    let last = presenter.data.as_ref().unwrap().cells.len() - 1;
    click_cell(&mut presenter, last);
    let copy = zone(&mut presenter, |zone| matches!(zone, Zone::CopyRow { .. }));
    assert_eq!(
        presenter.clicked(copy.mid_x(), copy.mid_y()),
        vec![WinAction::CopyReviewRequest {
            title: "Fix the resize test".into(),
            url: "https://github.com/acme/webapp/pull/7".into(),
        }]
    );
    let row = zone(&mut presenter, |zone| matches!(zone, Zone::OpenRow { .. }));
    assert_eq!(
        presenter.clicked(row.x + 4.0, row.mid_y()),
        vec![WinAction::OpenUrl(
            "https://github.com/acme/webapp/pull/7".into()
        )]
    );
    assert!(presenter.pinned().is_some(), "a row keeps the popover open");
}

#[test]
fn a_click_answers_the_plan_on_screen_not_one_from_data_that_came_after_it() {
    let mut presenter = shown(read_pulls(ShowMode::Always));
    let last = presenter.data.as_ref().unwrap().cells.len() - 1;
    click_cell(&mut presenter, last);
    let row = zone(&mut presenter, |zone| matches!(zone, Zone::OpenRow { .. }));
    presenter.accept(pull_request(ShowMode::Always, 8, "Fix the resize tests"));
    assert_eq!(
        presenter.clicked(row.x + 4.0, row.mid_y()),
        vec![WinAction::OpenUrl(
            "https://github.com/acme/webapp/pull/7".into()
        )],
        "the row under the pointer is the one drawn there"
    );
}

#[test]
fn the_pointer_is_read_at_the_scale_the_rail_was_drawn_at() {
    let mut presenter = Presenter::new(&FixedAdvance);
    presenter.accept(data(ShowMode::Always));
    let mut drawn = display("d1", false);
    drawn.scale = 2.0;
    presenter.set_displays(vec![drawn.clone()]);
    let first = presenter.present(clock()).expect("plan").cells[0].rect;
    presenter.set_displays(vec![Display {
        scale: 1.5,
        ..drawn
    }]);
    let device = |points: f64| (points * 2.0).round() as i32;
    let pressed = Input::Pressed {
        x: device(first.mid_x()),
        y: device(first.y + first.h - 2.0),
    };
    assert!(presenter.input(pressed, Instant::now()).is_empty());
    assert_eq!(
        presenter.pinned(),
        CLAUDE,
        "a device pixel on the first cell as drawn pins the first cell"
    );
}

fn pinned_on_screen(data: RailData, cell: CellId) -> Presenter {
    let mut presenter = shown(data);
    let rect = presenter
        .present(clock())
        .expect("plan")
        .cells
        .iter()
        .find(|planned| planned.id == cell)
        .expect("the cell is on the rail")
        .rect;
    presenter.clicked(rect.mid_x(), rect.mid_y());
    presenter.present(clock());
    assert!(presenter.pinned().is_some(), "the setup pins a popover");
    presenter
}

#[test]
fn new_figures_in_the_same_rail_keep_the_open_popover() {
    let mut presenter = pinned_on_screen(read_pulls(ShowMode::Always), CLAUDE.unwrap());
    presenter.accept(pull_request(ShowMode::Always, 8, "Fix the resize tests"));
    presenter.present(clock());
    assert_eq!(presenter.pinned(), CLAUDE);
    assert_eq!(presenter.hover.active, CLAUDE);
}

#[test]
fn another_pull_request_list_keeps_the_open_pull_request_popover() {
    let mut presenter = pinned_on_screen(read_pulls(ShowMode::Always), CellId::PullRequests);
    let mut more = read_pulls(ShowMode::Always);
    more.settings.pull_requests.lists = vec![GithubList::Mine, GithubList::ReviewRequested];
    presenter.accept(more);
    presenter.present(clock());
    assert_eq!(
        presenter.pinned(),
        Some(CellId::PullRequests),
        "the rail and its cells are where they were"
    );
}

#[test]
fn a_display_change_that_moves_the_rail_closes_the_popover() {
    let mut presenter = pinned_on_screen(read_pulls(ShowMode::Always), CLAUDE.unwrap());
    presenter.set_displays(vec![Display {
        width: 2560.0,
        ..display("d1", false)
    }]);
    presenter.present(clock());
    assert_eq!(presenter.pinned(), None);
    assert_eq!(presenter.hover, Hover::default());
}

#[test]
fn a_show_mode_change_closes_the_popover() {
    let mut presenter = pinned_on_screen(read_pulls(ShowMode::Always), CLAUDE.unwrap());
    presenter.accept(read_pulls(ShowMode::OnHover));
    let plan = presenter.present(clock()).expect("plan");
    assert!(plan.pill.is_some() && plan.popover.is_none(), "collapsed");
    assert_eq!(presenter.pinned(), None);
}

#[test]
fn a_notch_shown_again_after_a_full_screen_app_starts_with_nothing_open() {
    let mut presenter = pinned_on_screen(read_pulls(ShowMode::Always), CLAUDE.unwrap());
    presenter.set_suppressed(true);
    assert!(presenter.present(clock()).is_none());
    presenter.set_suppressed(false);
    let plan = presenter.present(clock()).expect("plan");
    assert!(plan.popover.is_none(), "no popover comes back");
    assert_eq!(presenter.pinned(), None);
}

#[test]
fn a_click_on_a_cell_of_the_drawn_open_rail_pins_it_though_on_hover_settings_came_after() {
    let mut presenter = shown(data(ShowMode::Always));
    let cell = cell_rect(&mut presenter);
    presenter.accept(data(ShowMode::OnHover));
    assert!(presenter.clicked(cell.mid_x(), cell.mid_y()).is_empty());
    assert_eq!(
        presenter.pinned(),
        CLAUDE,
        "the rail on screen is open and the click landed on its cell"
    );
}

#[test]
fn the_cap_asks_for_the_show_mode_its_drawn_pin_offers() {
    let mut presenter = shown(data(ShowMode::OnHover));
    let pill = presenter
        .present(clock())
        .expect("plan")
        .pill
        .expect("collapsed");
    presenter.cursor_at(pill.mid_x(), pill.mid_y(), true, Instant::now());
    let drawn = presenter.present(clock()).expect("plan");
    assert!(!drawn.cap.pinned, "the open rail shows the on-hover pin");
    let cap = drawn.cap.rect;
    presenter.accept(data(ShowMode::Always));
    assert_eq!(
        presenter.clicked(cap.mid_x(), cap.mid_y()),
        vec![WinAction::SetShow(ShowMode::Always)],
        "the pin on screen offers to keep the notch shown"
    );
}

#[test]
fn a_pinned_popover_stays_while_the_pointer_leaves_its_cell_and_follows_it_to_another_cell() {
    let mut presenter = shown(read_pulls(ShowMode::Always));
    let last = presenter.data.as_ref().unwrap().cells.len() - 1;
    let cell = presenter.present(clock()).expect("plan").cells[last].rect;
    let now = Instant::now();
    presenter.cursor_at(cell.mid_x(), cell.mid_y(), true, now);
    presenter.clicked(cell.mid_x(), cell.mid_y());
    assert_eq!(presenter.pinned(), Some(CellId::PullRequests));
    let row = zone(&mut presenter, |zone| matches!(zone, Zone::OpenRow { .. }));
    presenter.cursor_at(row.x + 4.0, row.mid_y(), true, now);
    assert_eq!(
        presenter.pinned(),
        Some(CellId::PullRequests),
        "the pin holds while the pointer moves onto its own popover"
    );
    assert_eq!(presenter.hover.active, Some(CellId::PullRequests));
    let first = presenter.present(clock()).expect("plan").cells[0].rect;
    presenter.cursor_at(first.mid_x(), first.mid_y(), true, now);
    assert_eq!(
        (presenter.pinned(), presenter.hover.active),
        (CLAUDE, CLAUDE),
        "a pin follows the pointer onto another cell"
    );
}

fn weekly_resetting(at: &str) -> RailData {
    let claude = NotchProvider::current(vec![crate::limits::signed_in_card(
        AgentId::Claude,
        "acct",
        Reading {
            windows: vec![LimitWindowDto {
                id: "weekly".into(),
                label: "Weekly · all models".into(),
                kind: LimitWindowKind::Weekly,
                used_percent: 10.0,
                resets_at: Some(at.into()),
                window_seconds: None,
                observed_at: "2026-09-01T10:00:00Z".into(),
                role: None,
                starts_on_first_message: false,
            }],
            ..Reading::default()
        },
    )])
    .expect("a signed-in account");
    frame::rail(
        &settings(ShowMode::Always),
        [(AgentId::Claude, Some(claude), Vec::new())],
        None,
        None,
    )
}

#[test]
fn the_presented_popover_words_its_reset_in_the_zone_of_the_clock_it_is_given() {
    let mut presenter = pinned_on_screen(weekly_resetting("2099-01-01T00:00:00Z"), CLAUDE.unwrap());
    for (hours, note) in [(13, "Resets Thu 13:00"), (-13, "Resets Wed 11:00")] {
        let zone = chrono::FixedOffset::east_opt(hours * 3600).unwrap();
        let notes: Vec<String> = presenter
            .present(clock().with_timezone(&zone))
            .expect("plan")
            .popover
            .as_ref()
            .expect("the popover stays open")
            .entries
            .iter()
            .filter_map(|(item, _)| match item {
                crate::side_notch::plan::PopItem::Text(run) if run.text.starts_with("Resets") => {
                    Some(run.text.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(notes, [note], "UTC{hours:+}");
    }
}
