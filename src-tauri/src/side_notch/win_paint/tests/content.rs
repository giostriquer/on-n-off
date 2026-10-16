use super::*;

fn ring_label(card: ProviderLimitsDto) -> String {
    match cell_content(&CellData::Provider(projected(card))) {
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
fn reset_notes_never_double_space_the_hour() {
    for hour in 0..24 {
        let mut pending = window("w", "Weekly", LimitWindowKind::Weekly, 10.0);
        pending.resets_at = Some(format!("2099-01-01T{hour:02}:07:00Z"));
        let note = reset_note(&pending);
        assert!(
            !note.contains("  "),
            "the hour is not space-padded at {hour}: {note:?}"
        );
    }
}
#[test]
fn percent_labels_follow_the_mac_rounding_rules() {
    let content = cell_content(&CellData::Provider(claude_with(vec![window(
        "w",
        "Weekly - all models",
        LimitWindowKind::Weekly,
        0.4,
    )])));
    match content {
        CellContent::Provider { label, .. } => assert_eq!(label, "<1%", "a sliver is not zero"),
        _ => panic!("wrong content kind"),
    }
}
#[test]
fn a_reset_window_reads_as_zero_and_never_recites_its_spent_figure() {
    let mut expired = window("w", "Weekly - all models", LimitWindowKind::Weekly, 97.0);
    expired.resets_at = Some("2020-01-01T00:00:00Z".into());
    let content = cell_content(&CellData::Provider(claude_with(vec![expired.clone()])));
    match content {
        CellContent::Provider {
            label, headline, ..
        } => {
            assert_eq!(label, "0%", "the quota renewed at the reset");
            assert_eq!(
                headline.and_then(|quota| quota.percent),
                Some(0.0),
                "and the ring draws an empty arc rather than none at all"
            );
        }
        _ => panic!("wrong content kind"),
    }

    assert_eq!(
        reset_note(&expired),
        format!(
            "Reset {}",
            chrono::DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Local)
                .format("%a %-I:%M %p")
        ),
        "the note says when it renewed, not what it held before"
    );

    let (planned, _) = popover_render(claude_with(vec![expired]));
    let popover = planned.popover.as_ref().expect("the popover is open");
    let texts: Vec<&str> = popover
        .entries
        .iter()
        .filter_map(|(item, _)| match item {
            PopItem::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        texts.contains(&"0% Used"),
        "the popover says so too: {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text.contains("97")),
        "the spent cycle's figure appears nowhere: {texts:?}"
    );
}
#[test]
fn unreadable_pull_requests_keep_their_rows_hidden() {
    let pulls = PrCellData {
        status: GithubStatus::GhNotLoggedIn,
        hint: Some("Sign in with the GitHub CLI.".into()),
        stale: false,
        lists: vec![PrListData {
            id: GithubList::Mine,
            total: 1,
            items: vec![PrRowData {
                id: "n1".into(),
                number: 1,
                title: "A stale row from the snapshot".into(),
                url: "https://github.com/o/r/pull/1".into(),
                repo: "o/r".into(),
                is_draft: false,
                review_decision: None,
                ci: CiState::Success,
                merge_kind: None,
                context: None,
            }],
        }],
    };
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &settings(),
        &displays,
        &data(vec![CellData::PullRequests(pulls)]),
        Hover {
            active: Some(0),
            ..Hover::default()
        },
    )
    .expect("fits");
    let popover = planned.popover.as_ref().expect("the popover is open");
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
}
#[test]
fn pull_request_titles_wrap_at_the_weight_they_are_drawn_in() {
    let pulls = PrCellData {
        status: GithubStatus::Ok,
        hint: None,
        stale: false,
        lists: vec![PrListData {
            id: GithubList::Mine,
            total: 1,
            items: vec![PrRowData {
                id: "n1".into(),
                number: 1,
                title: "Fix the flaky resize test number 1 on Windows builds".into(),
                url: "https://github.com/o/r/pull/1".into(),
                repo: "o/r".into(),
                is_draft: false,
                review_decision: None,
                ci: CiState::Success,
                merge_kind: None,
                context: None,
            }],
        }],
    };
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &settings(),
        &displays,
        &data(vec![CellData::PullRequests(pulls)]),
        Hover {
            active: Some(0),
            ..Hover::default()
        },
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
    let pulls = PrCellData {
        status: GithubStatus::Ok,
        hint: None,
        stale: false,
        lists: vec![PrListData {
            id: GithubList::Mine,
            total: 1,
            items: vec![PrRowData {
                id: "n1".into(),
                number: 7,
                title: "Fix the flaky resize test on Windows".into(),
                url: "https://github.com/o/r/pull/7".into(),
                repo: "o/r".into(),
                is_draft: true,
                review_decision: None,
                ci: CiState::Success,
                merge_kind: None,
                context: None,
            }],
        }],
    };
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
    let cells = vec![CellData::Provider(claude), CellData::PullRequests(pulls)];
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

#[test]
fn conflict_band_only_marks_passing_prs_with_merge_conflicts() {
    for ci in [
        CiState::None,
        CiState::Pending,
        CiState::Success,
        CiState::Failure,
        CiState::Error,
    ] {
        for merge_kind in [
            None,
            Some(MergeKind::Conflicts),
            Some(MergeKind::Ready),
            Some(MergeKind::Behind),
            Some(MergeKind::Blocked),
        ] {
            let content = cell_content(&CellData::PullRequests(PrCellData {
                status: GithubStatus::Ok,
                hint: None,
                stale: false,
                lists: vec![PrListData {
                    id: GithubList::Mine,
                    total: 1,
                    items: vec![PrRowData {
                        id: "fixture".into(),
                        number: 1,
                        title: "Fixture".into(),
                        url: "https://github.com/o/r/pull/1".into(),
                        repo: "o/r".into(),
                        is_draft: false,
                        review_decision: None,
                        ci,
                        merge_kind,
                        context: None,
                    }],
                }],
            }));
            let CellContent::PullRequests { segments, .. } = content else {
                panic!("PR cell")
            };
            assert_eq!(segments[0].ci, ci);
            assert_eq!(
                segments[0].passing_with_conflicts,
                ci == CiState::Success && merge_kind == Some(MergeKind::Conflicts)
            );
        }
    }
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

fn popover_texts(provider: ProviderData) -> Vec<String> {
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

fn inner_ring(provider: ProviderData) -> Option<(InnerRing, QuotaView, (Color, Color))> {
    match cell_content(&CellData::Provider(provider)) {
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
    match cell_content(&CellData::Provider(member.clone())) {
        CellContent::Provider {
            label, headline, ..
        } => {
            assert_eq!(label, "31%", "the weekly stays the headline");
            assert_eq!(headline.and_then(|quota| quota.percent), Some(31.0));
        }
        _ => panic!("wrong content kind"),
    }
    assert_eq!(
        inner_ring(member),
        Some((
            InnerRing::WorkspaceShare,
            QuotaView {
                percent: Some(32.0),
                reached: false
            },
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
        .map(|(_, quota, _)| quota),
        Some(QuotaView {
            percent: Some(0.0),
            reached: false
        }),
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
        window("fable", "Weekly · Fable", LimitWindowKind::Model, 13.0),
    ]);
    let (ring, quota, colors) = inner_ring(claude).expect("an inner ring");
    assert_eq!(
        ring,
        InnerRing::Fable {
            window_id: "fable".into()
        }
    );
    assert_eq!(quota.percent, Some(13.0));
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

    let texts = popover_texts(projected(paused));
    assert!(
        texts.contains(&"Refresh paused. Last observed values below.".to_string()),
        "{texts:?}"
    );
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
const PERSONAL: Color = [192, 132, 252, 255];

fn pr_row(number: u64, repo: &str, context: Option<PrContext>) -> PrRowData {
    PrRowData {
        id: format!("n{number}"),
        number,
        title: format!("Fix #{number}"),
        url: format!("https://github.com/{repo}/pull/{number}"),
        repo: repo.into(),
        is_draft: false,
        review_decision: None,
        ci: CiState::Success,
        merge_kind: None,
        context,
    }
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

fn violet([r, g, b]: [f64; 3]) -> bool {
    b > 100.0 && r > g * 1.2 && b > g * 1.5
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
fn each_row_wears_its_work_or_personal_ink_on_a_tag_a_stripe_and_the_copy_icon() {
    let pulls = PrCellData {
        status: GithubStatus::Ok,
        hint: None,
        stale: false,
        lists: vec![PrListData {
            id: GithubList::Mine,
            total: 4,
            items: vec![
                pr_row(1, "acme/webapp", Some(PrContext::Work)),
                pr_row(2, "octocat/dotfiles", Some(PrContext::Personal)),
                pr_row(3, "acme/webapp", None),
                pr_row(
                    1234,
                    "octocat/a-repository-name-long-enough-to-run-into-the-ci-dot",
                    Some(PrContext::Personal),
                ),
            ],
        }],
    };
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &settings(),
        &displays,
        &data(vec![CellData::PullRequests(pulls)]),
        Hover {
            active: Some(0),
            ..Hover::default()
        },
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
    let tags: Vec<usize> = (0..texts.len())
        .filter(|&i| matches!(texts[i].0, "WORK" | "PERSONAL"))
        .collect();
    assert_eq!(
        tags.iter()
            .map(|&i| (texts[i].0, texts[i].4))
            .collect::<Vec<_>>(),
        [
            ("WORK", WORK),
            ("PERSONAL", PERSONAL),
            ("PERSONAL", PERSONAL)
        ],
        "the tags wear the helper's own inks, and an unread row has none"
    );
    let dots: Vec<R> = popover
        .entries
        .iter()
        .filter_map(|(item, rect)| matches!(item, PopItem::Dot { .. }).then_some(*rect))
        .collect();
    for &i in &tags {
        let (tag, tag_rect, tag_size, tag_weight, _) = texts[i];
        let (repo, repo_rect, repo_size, repo_weight, _) = texts[i + 1];
        assert!(
            repo_rect.x >= tag_rect.x + measure_weight(tag, tag_size, tag_weight, 1.0),
            "{repo:?} starts after its {tag} tag"
        );
        assert!(
            repo_rect.x + measure_weight(repo, repo_size, repo_weight, 1.0) <= dots[3].x,
            "{repo:?} stays clear of the CI dot"
        );
    }
    let long = texts[tags[2] + 1].0;
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
                hued_pixels(&pixmap, *rect, violet),
            )
        })
        .collect();
    assert!(
        painted[0].0 > 0 && painted[1].1 > 0 && painted[3].1 > 0,
        "the copy icons are painted in their row's ink: {painted:?}"
    );
    assert_eq!(painted[2], (0, 0), "an unread row's copy icon stays muted");
}
