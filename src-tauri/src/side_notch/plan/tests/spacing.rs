use super::*;
use crate::side_notch::model::GithubList;

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
fn a_notice_and_a_paused_line_each_end_ten_points_before_what_follows() {
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
    same(paused.y, notice.y + notice.h + 10.0);
    same(
        at(&popover, "Weekly · all models").y,
        paused.y + paused.h + 10.0,
    );
}

#[test]
fn badges_follow_the_repository_and_each_other_five_points_apart() {
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
        repo.x + width("acme/webapp #42", TextWeight::Regular) + 5.0,
    );
    same(behind.x, draft.x + width("Draft", TextWeight::Medium) + 5.0);
}

#[test]
fn a_pull_request_list_stacks_its_rows_six_points_apart_and_ends_ten_before_the_footer() {
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
    same(at(&popover, "Open Pull requests").y, last.y + last.h + 10.0);
}

#[test]
fn an_action_error_sits_ten_points_above_the_footer() {
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
    same(at(&popover, "Open Limits").y, error.y + error.h + 10.0);
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
    let figure = at(&popover, "41% Used");
    let divider = first(&popover, |item| matches!(item, PopItem::Divider));
    same(divider.y, figure.y + figure.h + 10.0);
    same(at(&popover, "first").y, divider.y + 1.0 + 10.0);
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

#[test]
fn a_pull_request_notice_and_each_list_end_ten_points_before_what_follows() {
    let dto = prs_dto(
        "rateLimited",
        Some("GitHub is rate limiting requests."),
        true,
        1,
        vec![pr(1, "Fix one", "acme/webapp")],
    );
    let only = NotchSettings {
        providers: Vec::new(),
        pull_requests: NotchPullRequests {
            enabled: true,
            lists: vec![GithubList::Mine, GithubList::ReviewRequested],
        },
        ..settings()
    };
    let popover = opened(decided(only, Vec::new(), Some(&dto)));
    let notice = at(&popover, "GitHub is rate limiting requests.");
    let mine = at(&popover, "MINE");
    same(mine.y, notice.y + notice.h + 10.0);
    let last = at(&popover, "acme/webapp #1");
    same(at(&popover, "REVIEW REQUESTED").y, last.y + last.h + 10.0);
}

#[test]
fn the_ci_dot_centres_in_its_twenty_point_frame_eight_after_the_repository_line() {
    let popover = opened(pulls(vec![pr(1, "Fix one", "acme/webapp")]));
    let card = popover.card;
    let dot = first(&popover, |item| matches!(item, PopItem::Dot { .. }));
    let padding = crate::side_notch::model::popover_metrics(
        NotchSettings::default().size,
        &display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0),
    )
    .padding;
    let right = card.x + card.w - padding;
    same(dot.mid_x(), right - 10.0);
    let long = opened(pulls(vec![pr(
        1,
        "Fix one",
        "octocat/a-repository-name-long-enough-to-run-into-the-ci-dot",
    )]));
    let repo = long
        .entries
        .iter()
        .find_map(|(item, rect)| match item {
            PopItem::Text(text) if text.text.starts_with("octocat/") => Some((text.clone(), *rect)),
            _ => None,
        })
        .expect("the repository line is planned");
    let width = measure_weight(&repo.0.text, repo.0.size, repo.0.weight, 1.0);
    assert!(
        repo.1.x + width <= right - 20.0 - 8.0,
        "the repository line ends 8 pt before the dot's frame: {} ends at {}",
        repo.0.text,
        repo.1.x + width
    );
}
