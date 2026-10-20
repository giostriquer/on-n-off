use super::*;

fn ring_label(card: ProviderLimitsDto) -> String {
    match cell_content(&CellData::Provider(projected(card)), now()) {
        CellContent::Provider { label, .. } => label,
        _ => panic!("wrong content kind"),
    }
}

#[test]
fn the_ring_label_is_the_weekly_figure_or_a_dash_never_the_session() {
    let weekly = window(
        "weekly_all",
        "Weekly · all models",
        LimitWindowKind::Weekly,
        41.0,
    );
    let session = window(
        "session",
        "5 hour · all models",
        LimitWindowKind::Session,
        73.0,
    );
    assert_eq!(
        ring_label(signed_in(AgentId::Claude, vec![weekly, session.clone()])),
        "41%"
    );
    assert_eq!(
        ring_label(signed_in(AgentId::Claude, vec![session])),
        "—",
        "without a weekly window the ring leads with nothing, never the session"
    );
}

#[test]
fn unreadable_pull_requests_keep_their_rows_hidden() {
    let pulls = pulls_read(
        "ghNotLoggedIn",
        Some("Sign in with the GitHub CLI."),
        1,
        vec![pr(1, "A stale row from the snapshot", "o/r")],
    );
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &settings(),
        &displays,
        &data(vec![pulls]),
        Hover {
            active: Some(0),
            ..Hover::default()
        },
        now(),
    )
    .expect("fits");
    let popover = planned.popover.as_ref().expect("the popover is open");
    assert!(
        inks(&planned).contains(&("Sign in with the GitHub CLI.".to_string(), MUTED_INK)),
        "an unreadable read shows its hint, muted"
    );
    assert!(
        !popover.entries.iter().any(
            |(item, _)| matches!(item, PopItem::Text { text, .. } if text.contains("stale row"))
        ),
        "an unreadable list shows the hint, not rows"
    );
    assert!(
        popover
            .zones
            .iter()
            .all(|(zone, _)| !matches!(zone, Zone::OpenRow { .. })),
        "and offers no row actions"
    );
}
#[test]
fn a_long_session_name_is_ellipsized_not_chopped() {
    let mut provider = claude_with(vec![window(
        "w",
        "Weekly - all models",
        LimitWindowKind::Weekly,
        50.0,
    )]);
    provider.sessions = vec![LiveSession {
        id: "s1".into(),
        name: "a-very-long-running-session-name-that-cannot-possibly-fit".into(),
        place: "Terminal".into(),
        project: "on-n-off".into(),
        status: crate::side_notch::sessions::SessionStatus::Working,
        last_active_at: "2026-09-03T10:00:00Z".into(),
    }];
    let (planned, _) = popover_render(provider);
    let popover = planned.popover.as_ref().expect("the popover is open");
    let name = popover
        .entries
        .iter()
        .find_map(|(item, _)| match item {
            PopItem::Text { text, .. } if text.starts_with("a-very-long") => Some(text.clone()),
            _ => None,
        })
        .expect("the session name is planned");
    assert!(name.ends_with('…'), "a cut name says it was cut: {name:?}");
    assert!(
        popover
            .entries
            .iter()
            .any(|(item, _)| matches!(item, PopItem::Text { text, .. } if text == "30 min")),
        "the session's age is read against the clock the plan was given"
    );
}
#[test]
fn pull_request_titles_wrap_at_the_weight_they_are_drawn_in() {
    let pulls = pulls(vec![pr(
        1,
        "Fix the flaky resize test number 1 on Windows builds",
        "o/r",
    )]);
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &settings(),
        &displays,
        &data(vec![pulls]),
        Hover {
            active: Some(0),
            ..Hover::default()
        },
        now(),
    )
    .expect("fits");
    let popover = planned.popover.as_ref().expect("the popover is open");
    for (item, rect) in &popover.entries {
        let PopItem::Text {
            text, size, weight, ..
        } = item
        else {
            continue;
        };
        if !text.starts_with("Fix the flaky") && !text.starts_with("Windows builds") {
            continue;
        }
        assert_eq!(*weight, TextWeight::Semibold, "titles are semibold");
        let drawn = measure_weight(text, *size, *weight, planned.display_scale);
        assert!(
            drawn <= rect.w,
            "the wrapped line fits its box: {text:?} is {drawn} wide in {}",
            rect.w
        );
    }
}

#[test]
fn the_popover_keeps_the_mac_weight_of_every_run() {
    let mut draft = pr(7, "Fix the flaky resize test on Windows", "o/r");
    draft["isDraft"] = true.into();
    let pulls = pulls(vec![draft]);
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let mut claude = claude_with(vec![window(
        "w",
        "Weekly - all models",
        LimitWindowKind::Weekly,
        50.0,
    )]);
    claude.sessions = vec![LiveSession {
        id: "s1".into(),
        name: "notch".into(),
        place: "Terminal".into(),
        project: "on-n-off".into(),
        status: crate::side_notch::sessions::SessionStatus::Working,
        last_active_at: "2026-09-03T10:00:00Z".into(),
    }];
    let cells = vec![CellData::Provider(claude), pulls];
    let medium = [
        "50% Used",
        "working",
        "Open Limits",
        "Open Pull requests",
        "Draft",
    ];
    let semibold = [
        "Claude Usage",
        "Weekly - all models",
        "notch",
        "Pull requests",
        "MINE",
    ];
    let mut seen = 0;
    for index in 0..cells.len() {
        let planned = plan(
            &settings(),
            &displays,
            &data(cells.clone()),
            Hover {
                active: Some(index),
                ..Hover::default()
            },
            now(),
        )
        .expect("fits");
        let popover = planned.popover.as_ref().expect("the popover is open");
        for (item, _) in &popover.entries {
            let PopItem::Text { text, weight, .. } = item else {
                continue;
            };
            if medium.contains(&text.as_str()) {
                assert_eq!(*weight, TextWeight::Medium, "{text:?} is a medium run");
                seen += 1;
            }
            if semibold.contains(&text.as_str()) {
                assert_eq!(*weight, TextWeight::Semibold, "{text:?} is a semibold run");
                seen += 1;
            }
        }
    }
    assert!(
        seen >= 8,
        "the popovers really carried these runs: {seen} of 10"
    );
    let plain = measure_weight("Open Limits", 11.0, TextWeight::Regular, 1.0);
    let heavy = measure_weight("Open Limits", 11.0, TextWeight::Medium, 1.0);
    assert!(
        heavy > plain,
        "a medium run is heavier than a regular one: {plain} vs {heavy}"
    );
}

fn share(
    used: &str,
    used_percent: f64,
    reached: bool,
    resets_at: &str,
) -> LimitsWorkspaceCreditsDto {
    LimitsWorkspaceCreditsDto {
        limit: "25000".into(),
        used: used.into(),
        used_percent,
        resets_at: Some(resets_at.into()),
        reached,
    }
}

fn popover_texts(provider: ProviderCell) -> Vec<String> {
    let (planned, _) = popover_render(provider);
    let popover = planned.popover.expect("the popover is open");
    popover
        .entries
        .into_iter()
        .filter_map(|(item, _)| match item {
            PopItem::Text { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

fn inner_ring(provider: ProviderCell) -> Option<(InnerRing, WindowAt, (Color, Color))> {
    match cell_content(&CellData::Provider(provider), now()) {
        CellContent::Provider { inner, .. } => inner.map(|(ring, quota)| {
            let colors = inner_ring_colors(&ring);
            (ring, quota, colors)
        }),
        _ => panic!("wrong content kind"),
    }
}

#[test]
fn a_codex_members_credit_share_fills_the_inner_ring_under_the_weekly() {
    let member = codex_member(share("8000", 32.0, false, "2099-01-01T12:00:00Z"));
    match cell_content(&CellData::Provider(member.clone()), now()) {
        CellContent::Provider {
            label, headline, ..
        } => {
            assert_eq!(label, "31%", "the weekly stays the headline");
            assert_eq!(headline.and_then(|at| at.used_now()), Some(31.0));
        }
        _ => panic!("wrong content kind"),
    }
    assert_eq!(
        inner_ring(member).map(|(ring, at, colors)| (ring, at.used_now(), colors)),
        Some((
            InnerRing::WorkspaceShare,
            Some(32.0),
            (CREDITS_INK, CREDITS_TRACK),
        ))
    );
    assert_eq!(
        inner_ring(codex_member(share(
            "25000",
            100.0,
            true,
            "2020-01-01T12:00:00Z"
        )))
        .map(|(_, at, _)| at.used_now()),
        Some(Some(0.0)),
        "a share past its reset has renewed, as a window has"
    );
}

#[test]
fn claudes_fable_window_fills_the_inner_ring_in_its_own_ink() {
    let claude = claude_with(vec![
        window(
            "weekly",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            7.0,
        ),
        LimitWindowDto {
            role: Some(crate::dto::WindowRole::InnerRing),
            ..window("fable", "Weekly · Fable", LimitWindowKind::Model, 13.0)
        },
    ]);
    let (ring, at, colors) = inner_ring(claude).expect("an inner ring");
    assert_eq!(
        ring,
        InnerRing::Fable {
            window_id: "fable".into()
        }
    );
    assert_eq!(at.used_now(), Some(13.0));
    assert_eq!(colors, (FABLE_ORANGE, FABLE_TRACK));
}

#[test]
fn the_codex_popover_lists_the_credit_share_after_the_weekly() {
    let texts = popover_texts(codex_member(share(
        "8000",
        32.0,
        false,
        "2099-01-01T12:00:00Z",
    )));
    let weekly = texts.iter().position(|text| text == "Weekly · all models");
    let credits = texts.iter().position(|text| text == "Workspace credits");
    assert!(weekly.is_some() && credits > weekly, "{texts:?}");
    let note = format!(
        "Resets {}",
        chrono::DateTime::parse_from_rfc3339("2099-01-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Local)
            .format("%b %-d")
    );
    assert_eq!(texts.get(credits.unwrap() + 1), Some(&note), "{texts:?}");
    assert!(
        texts.contains(&"32% Used · 17,000 of 25,000 left".to_string()),
        "{texts:?}"
    );
    let (planned, _) = popover_render(codex_member(share(
        "8000",
        32.0,
        false,
        "2099-01-01T12:00:00Z",
    )));
    let bars = planned
        .popover
        .expect("the popover is open")
        .entries
        .iter()
        .filter(|(item, _)| matches!(item, PopItem::Bar { .. }))
        .count();
    assert_eq!(bars, 2, "a bar for the weekly and one for the share");
}

#[test]
fn the_codex_popover_words_a_reached_and_a_renewed_share_as_the_app_does() {
    let reached = popover_texts(codex_member(share(
        "24000",
        100.0,
        true,
        "2099-01-01T12:00:00Z",
    )));
    assert!(
        reached.contains(&"100% Used · limit reached".to_string()),
        "{reached:?}"
    );
    let renewed = popover_texts(codex_member(share(
        "25000",
        100.0,
        true,
        "2020-01-01T12:00:00Z",
    )));
    assert!(
        renewed.contains(&"0% Used · 25,000 of 25,000 left".to_string()),
        "{renewed:?}"
    );
}

#[test]
fn a_paused_account_with_only_a_share_says_its_values_are_last_observed() {
    let mut paused = codex_member_card(share("8000", 32.0, false, "2099-01-01T12:00:00Z"));
    paused.status = LimitsStatus::Failed;
    paused.message = Some("Refresh failed.".into());
    paused.reading.windows.clear();

    let (planned, _) = popover_render(projected(paused));
    let inks = inks(&planned);
    assert!(
        inks.contains(&("Refresh failed.".to_string(), MUTED_INK)),
        "{inks:?}"
    );
    assert!(
        inks.contains(&(
            "Refresh paused. Last observed values below.".to_string(),
            WARN_AMBER
        )),
        "{inks:?}"
    );
}

fn inks(planned: &Plan) -> Vec<(String, Color)> {
    planned
        .popover
        .as_ref()
        .expect("the popover is open")
        .entries
        .iter()
        .filter_map(|(item, _)| match item {
            PopItem::Text { text, color, .. } => Some((text.clone(), *color)),
            _ => None,
        })
        .collect()
}

#[test]
fn each_badge_wears_the_ink_of_its_tone() {
    let mut drafted = pr(1, "Fix one", "o/r");
    drafted["isDraft"] = true.into();
    drafted["mergeKind"] = "behind".into();
    let mut rejected = pr(2, "Fix two", "o/r");
    rejected["reviewDecision"] = "CHANGES_REQUESTED".into();
    let mut approved = pr(3, "Fix three", "o/r");
    approved["reviewDecision"] = "APPROVED".into();
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &settings(),
        &displays,
        &data(vec![pulls(vec![drafted, rejected, approved])]),
        Hover {
            active: Some(0),
            ..Hover::default()
        },
        now(),
    )
    .expect("fits");
    let inks = inks(&planned);
    for badge in [
        ("Draft", MUTED_INK),
        ("Behind base", WARN_AMBER),
        ("Changes requested", TRIP_RED),
        ("Approved", LIVE_GREEN),
    ] {
        assert!(
            inks.contains(&(badge.0.to_string(), badge.1)),
            "{badge:?} in {inks:?}"
        );
    }
}

#[test]
fn a_paused_account_keeps_its_last_reading_on_the_ring() {
    let paused = |mut card: ProviderLimitsDto| {
        card.status = LimitsStatus::Failed;
        card.message = Some("Claude Code reported no usage.".into());
        card
    };
    let weekly = window(
        "weekly_all",
        "Weekly · all models",
        LimitWindowKind::Weekly,
        46.0,
    );

    assert_eq!(
        ring_label(paused(signed_in(AgentId::Claude, vec![weekly]))),
        "46%"
    );
    assert!(inner_ring(projected(paused(codex_member_card(share(
        "8000",
        32.0,
        false,
        "2099-01-01T12:00:00Z"
    )))))
    .is_some());
    assert_eq!(
        ring_label(paused(signed_in(
            AgentId::Cursor,
            vec![session_window(50.0)]
        ))),
        "—"
    );
}

const WORK: Color = [96, 165, 250, 255];
const PERSONAL: Color = [170, 155, 215, 255];

fn pr_row(number: u64, repo: &str, in_organization: Option<bool>) -> serde_json::Value {
    let mut row = pr(number, &format!("Fix #{number}"), repo);
    row["inOrganization"] = serde_json::to_value(in_organization).unwrap();
    row
}

fn longest_run(pixmap: &tiny_skia::Pixmap, ink: Color) -> u32 {
    let mut longest = 0;
    for x in 0..pixmap.width() {
        let mut run = 0;
        for y in 0..pixmap.height() {
            let hit = pixmap
                .pixel(x, y)
                .is_some_and(|px| [px.red(), px.green(), px.blue(), px.alpha()] == ink);
            run = if hit { run + 1 } else { 0 };
            longest = longest.max(run);
        }
    }
    longest
}

fn bluish([r, g, b]: [f64; 3]) -> bool {
    b > 100.0 && b > r * 1.4 && b > g * 1.15
}

fn lavender([r, g, b]: [f64; 3]) -> bool {
    b > 100.0 && b > r && r > g * 1.05 && b > g * 1.2
}

fn hued_pixels(pixmap: &tiny_skia::Pixmap, rect: R, hue: fn([f64; 3]) -> bool) -> usize {
    let xs = rect.x.floor() as u32..(rect.x + rect.w).ceil() as u32;
    let ys = rect.y.floor() as u32..(rect.y + rect.h).ceil() as u32;
    ys.flat_map(|y| xs.clone().map(move |x| (x, y)))
        .filter(|&(x, y)| {
            pixmap.pixel(x, y).is_some_and(|px| {
                hue([
                    f64::from(px.red()),
                    f64::from(px.green()),
                    f64::from(px.blue()),
                ])
            })
        })
        .count()
}

#[test]
fn each_row_wears_its_work_or_personal_ink_on_a_stripe_and_the_copy_icon_with_no_tag() {
    let mut approved = pr_row(1, "acme/webapp", Some(true));
    approved["reviewDecision"] = "APPROVED".into();
    let pulls = pulls(vec![
        approved,
        pr_row(2, "octocat/dotfiles", Some(false)),
        pr_row(3, "acme/webapp", None),
        pr_row(
            1234,
            "octocat/a-repository-name-long-enough-to-run-into-the-ci-dot",
            Some(false),
        ),
    ]);
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &settings(),
        &displays,
        &data(vec![pulls]),
        Hover {
            active: Some(0),
            ..Hover::default()
        },
        now(),
    )
    .expect("fits");
    let popover = planned.popover.as_ref().expect("the popover is open");
    let texts: Vec<(&str, R, f64, TextWeight, Color)> = popover
        .entries
        .iter()
        .filter_map(|(item, rect)| match item {
            PopItem::Text {
                text,
                size,
                weight,
                color,
                ..
            } => Some((text.as_str(), *rect, *size, *weight, *color)),
            _ => None,
        })
        .collect();
    assert!(
        texts
            .iter()
            .all(|(text, ..)| !matches!(*text, "WORK" | "PERSONAL")),
        "no row carries a WORK or PERSONAL tag"
    );
    let titles: Vec<R> = texts
        .iter()
        .filter(|(text, ..)| text.starts_with("Fix #"))
        .map(|(_, rect, ..)| *rect)
        .collect();
    assert_eq!(titles.len(), 4, "one title line per row: {titles:?}");
    let repos: Vec<(&str, R, f64, TextWeight, Color)> = texts
        .iter()
        .copied()
        .filter(|(text, ..)| text.starts_with("acme/") || text.starts_with("octocat/"))
        .collect();
    assert_eq!(repos.len(), 4, "one repository line per row: {repos:?}");
    let dots: Vec<R> = popover
        .entries
        .iter()
        .filter_map(|(item, rect)| matches!(item, PopItem::Dot { .. }).then_some(*rect))
        .collect();
    for ((repo, repo_rect, repo_size, repo_weight, color), title) in repos.iter().zip(&titles) {
        assert_eq!(
            repo_rect.x, title.x,
            "{repo:?} starts under its title, with nothing before it"
        );
        assert_eq!(*color, MUTED_INK, "{repo:?} keeps the muted ink");
        assert!(
            repo_rect.x + measure_weight(repo, *repo_size, *repo_weight, 1.0) <= dots[3].x,
            "{repo:?} stays clear of the CI dot"
        );
    }
    let (repo, repo_rect, repo_size, repo_weight, _) = repos[0];
    let (badge, badge_rect, badge_size, badge_weight, _) = *texts
        .iter()
        .find(|(text, ..)| *text == "Approved")
        .expect("the first row shows its review decision");
    assert_eq!(
        badge_rect.y, repo_rect.y,
        "the badge shares its repository's line"
    );
    assert!(
        badge_rect.x >= repo_rect.x + measure_weight(repo, repo_size, repo_weight, 1.0),
        "the badge follows {repo:?} instead of overlapping it"
    );
    assert!(
        badge_rect.x + measure_weight(badge, badge_size, badge_weight, 1.0) <= dots[3].x,
        "the badge stays clear of the CI dot"
    );
    let long = repos[3].0;
    assert!(
        long.starts_with("octocat/a-repository") && long.ends_with('…'),
        "a repository too long for the line is ellipsized, not run under the dot: {long:?}"
    );
    let copies: Vec<(Color, R)> = popover
        .entries
        .iter()
        .filter_map(|(item, rect)| match item {
            PopItem::CopyIcon { color } => Some((*color, *rect)),
            _ => None,
        })
        .collect();
    assert_eq!(
        copies.iter().map(|(color, _)| *color).collect::<Vec<_>>(),
        [WORK, PERSONAL, MUTED_INK, PERSONAL]
    );
    let stripes: Vec<(Color, R)> = popover
        .entries
        .iter()
        .filter_map(|(item, rect)| match item {
            PopItem::Stripe { color } => Some((*color, *rect)),
            _ => None,
        })
        .collect();
    let rows: Vec<R> = popover
        .zones
        .iter()
        .filter_map(|(zone, rect)| matches!(zone, Zone::OpenRow { .. }).then_some(*rect))
        .collect();
    assert_eq!(
        stripes.iter().map(|(color, _)| *color).collect::<Vec<_>>(),
        [WORK, PERSONAL, PERSONAL]
    );
    for ((_, stripe), row) in stripes.iter().zip([rows[0], rows[1], rows[3]]) {
        assert_eq!(
            (stripe.y, stripe.h),
            (row.y, row.h),
            "the stripe runs the whole row"
        );
        assert!(
            stripe.x + stripe.w < row.x,
            "and sits in the gutter, clear of the text"
        );
    }
    let pixmap = render(&planned);
    for ink in [WORK, PERSONAL] {
        assert!(
            longest_run(&pixmap, ink) >= 20,
            "the painted popover shows an unbroken {ink:?} stripe"
        );
    }
    let painted: Vec<(usize, usize)> = copies
        .iter()
        .map(|(_, rect)| {
            (
                hued_pixels(&pixmap, *rect, bluish),
                hued_pixels(&pixmap, *rect, lavender),
            )
        })
        .collect();
    assert!(
        painted[0].0 > 0 && painted[1].1 > 0 && painted[3].1 > 0,
        "the copy icons are painted in their row's ink: {painted:?}"
    );
    assert_eq!(painted[2], (0, 0), "an unread row's copy icon stays muted");
}

#[test]
fn a_cell_not_read_yet_keeps_its_slot_with_a_dash_and_says_it_is_checking() {
    let selected = NotchSettings {
        providers: vec![AgentId::Claude, AgentId::Codex],
        ..settings()
    };
    let claude = NotchProvider::current(vec![signed_in(
        AgentId::Claude,
        vec![window(
            "weekly",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            41.0,
        )],
    )]);
    let data = frame::rail(
        &selected,
        [(AgentId::Claude, claude, Vec::new())],
        None,
        None,
    );
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let rail = plan(&selected, &displays, &data, Hover::default(), now()).expect("fits");
    let labels: Vec<String> = rail
        .cells
        .iter()
        .map(|cell| match &cell.content {
            CellContent::Provider { label, .. } => label.clone(),
            CellContent::PullRequests { label, segments } => {
                assert!(
                    segments.is_empty(),
                    "nothing about the pull requests is known yet"
                );
                label.clone()
            }
        })
        .collect();
    assert_eq!(
        labels,
        ["41%", "—", "—"],
        "Codex and the pull requests keep their slots until they are read"
    );
    for (index, checking) in [(1, "Checking usage…"), (2, "Checking pull requests…")] {
        let planned = plan(
            &selected,
            &displays,
            &data,
            Hover {
                active: Some(index),
                ..Hover::default()
            },
            now(),
        )
        .expect("fits");
        let texts: Vec<String> = planned
            .popover
            .expect("the popover is open")
            .entries
            .into_iter()
            .filter_map(|(item, _)| match item {
                PopItem::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&checking.to_string()), "{texts:?}");
    }
}

#[test]
fn a_read_pull_request_cell_draws_the_figure_and_segments_the_model_decided() {
    let mut conflicted = pr(1, "Fix the first thing", "o/r");
    conflicted["mergeKind"] = "conflicts".into();
    let mut failing = pr(2, "Fix the second thing", "o/r");
    failing["ci"] = "failure".into();
    let cell = pulls(vec![conflicted, failing]);
    let CellData::PullRequests(decided) = &cell else {
        panic!("a pull request cell");
    };
    let segments = decided.reading.as_ref().expect("read").segments.clone();
    assert_eq!(
        segments,
        [
            PrSegment {
                ci: CiState::Success,
                passing_with_conflicts: true
            },
            PrSegment {
                ci: CiState::Failure,
                passing_with_conflicts: false
            },
        ]
    );
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &settings(),
        &displays,
        &data(vec![cell.clone()]),
        Hover::default(),
        now(),
    )
    .expect("fits");
    assert_eq!(
        planned.cells[0].content,
        CellContent::PullRequests {
            label: "2".into(),
            segments
        },
        "the ring draws the frame's own figure and segments"
    );
}
