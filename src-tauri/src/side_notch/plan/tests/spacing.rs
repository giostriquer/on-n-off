use super::*;

fn at(popover: &PopoverPlan, wanted: &str) -> R {
    popover
        .entries
        .iter()
        .find_map(|(item, rect)| match item {
            PopItem::Text(Text { text, right, .. }) if text == wanted && !right => Some(*rect),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{wanted:?} is planned"))
}

fn first(popover: &PopoverPlan, kind: fn(&PopItem) -> bool) -> R {
    popover
        .entries
        .iter()
        .find_map(|(item, rect)| kind(item).then_some(*rect))
        .expect("the item is planned")
}

fn opened(cell: CellData) -> PopoverPlan {
    let hover = open_on(CellId::of(&cell));
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    plan(&displays, &data(vec![cell]), hover, now())
        .expect("fits")
        .popover
        .expect("the popover is open")
}

#[track_caller]
fn same(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{actual} is not {expected}"
    );
}

#[test]
fn the_card_stacks_its_sections_ten_points_apart_and_each_quota_block_five() {
    let popover = opened(CellData::Provider(claude_with(vec![
        window(
            "weekly",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            41.0,
        ),
        window(
            "session",
            "5 hour · all models",
            LimitWindowKind::Session,
            73.0,
        ),
    ])));
    let card = popover.card;
    let title = at(&popover, "Claude Usage");
    same(title.y - card.y, 12.0);
    let weekly = at(&popover, "Weekly · all models");
    same(weekly.y, title.y + title.h + 10.0);
    let bar = first(&popover, |item| matches!(item, PopItem::Bar { .. }));
    same(bar.y, weekly.y + weekly.h + 5.0);
    let figure = at(&popover, "41% Used");
    same(figure.y, bar.y + bar.h + 5.0);
    same(
        at(&popover, "5 hour · all models").y,
        figure.y + figure.h + 10.0,
    );
}

#[test]
fn a_paused_line_follows_its_notice_and_sits_four_points_above_the_first_window() {
    let mut card = signed_in(
        AgentId::Codex,
        vec![window(
            "weekly",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            88.0,
        )],
    );
    card.status = LimitsStatus::Failed;
    card.message = Some("Refresh failed.".into());
    let popover = opened(CellData::Provider(projected(card)));
    let notice = at(&popover, "Refresh failed.");
    let paused = at(&popover, "Refresh paused. Last observed values below.");
    same(paused.y, notice.y + notice.h);
    same(
        at(&popover, "Weekly · all models").y,
        paused.y + paused.h + 4.0,
    );
}

#[test]
fn badges_follow_the_repository_eight_points_on_and_sit_six_apart() {
    let mut row = pr(42, "Fix the resize test", "acme/webapp");
    row["isDraft"] = true.into();
    row["mergeKind"] = "behind".into();
    let popover = opened(pulls(vec![row]));
    let repo = at(&popover, "acme/webapp #42");
    let draft = at(&popover, "Draft");
    let behind = at(&popover, "Behind base");
    let width = |text: &str, weight| measure_weight(text, 10.0, weight, 1.0);
    assert_eq!((draft.y, behind.y), (repo.y, repo.y), "one line");
    same(
        draft.x,
        repo.x + width("acme/webapp #42", TextWeight::Regular) + 8.0,
    );
    same(behind.x, draft.x + width("Draft", TextWeight::Medium) + 6.0);
}

#[test]
fn a_pull_request_list_stacks_its_rows_six_points_apart_and_ends_fourteen_before_the_footer() {
    let popover = opened(pulls(vec![
        pr(1, "Fix one", "acme/webapp"),
        pr(2, "Fix two", "acme/webapp"),
    ]));
    let title = at(&popover, "MINE");
    let one = at(&popover, "Fix one");
    same(one.y, title.y + title.h + 6.0);
    let repo = at(&popover, "acme/webapp #1");
    same(repo.y, one.y + one.h + 2.0);
    same(at(&popover, "Fix two").y, repo.y + repo.h + 6.0);
    let last = at(&popover, "acme/webapp #2");
    same(
        at(&popover, "Open Pull requests").y,
        last.y + last.h + 6.0 + 8.0,
    );
}

#[test]
fn an_action_error_sits_right_above_the_footer() {
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let mut rail = data(vec![CellData::Provider(session_only(
        AgentId::Claude,
        42.0,
    ))]);
    rail.action_error = Some("Could not open the pull request.".into());
    let popover = plan(&displays, &rail, open_on(CLAUDE), now())
        .expect("fits")
        .popover
        .expect("the popover is open");
    let error = at(&popover, "Could not open the pull request.");
    same(at(&popover, "Open Limits").y, error.y + error.h);
}

#[test]
fn a_session_stacks_its_lines_two_apart_and_the_list_ends_ten_above_the_footer() {
    let mut provider = claude_with(vec![window(
        "weekly",
        "Weekly · all models",
        LimitWindowKind::Weekly,
        41.0,
    )]);
    let session = |name: &str| LiveSession {
        id: name.into(),
        name: name.into(),
        place: "Terminal".into(),
        project: "webapp".into(),
        status: crate::side_notch::sessions::SessionStatus::Idle,
        last_active_at: "2026-09-03T10:00:00Z".into(),
    };
    provider.sessions = vec![session("first"), session("second")];
    let popover = opened(CellData::Provider(provider));
    let first = at(&popover, "first");
    let place = at(&popover, "Terminal · webapp");
    same(place.y, first.y + first.h + 2.0);
    let second = at(&popover, "second");
    same(second.y, place.y + place.h + 8.0);
    let last = popover
        .entries
        .iter()
        .rev()
        .find_map(|(item, rect)| match item {
            PopItem::Text(Text { text, right, .. }) if text == "Terminal · webapp" && !right => {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the second session's place");
    same(at(&popover, "Open Limits").y, last.y + last.h + 10.0);
}
