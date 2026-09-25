use super::*;

/// How many windows the popover of a Codex account with `windows` draws a bar for.
fn codex_popover_bars(windows: Vec<LimitWindowDto>) -> usize {
    let (planned, _) = popover_render(ProviderData {
        provider: AgentId::Codex,
        status: LimitsStatus::Ok,
        message: None,
        windows,
        workspace_credits: None,
        sessions: Vec::new(),
    });
    planned
        .popover
        .expect("the popover is open")
        .entries
        .iter()
        .filter(|(item, _)| matches!(item, PopItem::Bar { .. }))
        .count()
}

#[test]
fn codex_hides_its_internal_buckets_by_id_whatever_their_label() {
    let bars = codex_popover_bars(vec![
        window("w-session", "Session", LimitWindowKind::Session, 10.0),
        window(
            "extra:base_model_inference",
            "Weekly · Inference",
            LimitWindowKind::Model,
            90.0,
        ),
        window(
            "extra:codex_bengalfox:secondary",
            "Weekly · Bengal preview",
            LimitWindowKind::Model,
            90.0,
        ),
    ]);
    assert_eq!(bars, 1, "only the session window reaches the popover");
}

#[test]
fn codex_hides_the_reserve_and_spark_windows_by_label_whatever_their_id() {
    let bars = codex_popover_bars(vec![
        window("w-session", "Session", LimitWindowKind::Session, 10.0),
        window(
            "extra:spark",
            "Weekly · gpt-5.3-codex-spark",
            LimitWindowKind::Model,
            90.0,
        ),
        window(
            "extra:reserve",
            "Weekly · GPT-Reserve",
            LimitWindowKind::Model,
            90.0,
        ),
    ]);
    assert_eq!(bars, 1, "only the session window reaches the popover");
}

/// The label of a cell whose account reports `windows`.
fn ring_label(provider: AgentId, windows: Vec<LimitWindowDto>) -> String {
    let mut data = claude_with(windows);
    data.provider = provider;
    match cell_content(&CellData::Provider(data)) {
        CellContent::Provider { label, .. } => label,
        _ => panic!("wrong content kind"),
    }
}

#[test]
fn claudes_ring_leads_with_its_weekly_over_its_session() {
    let label = ring_label(
        AgentId::Claude,
        vec![
            window(
                "session",
                "5 hour · all models",
                LimitWindowKind::Session,
                73.0,
            ),
            window(
                "weekly_all",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                41.0,
            ),
        ],
    );
    assert_eq!(label, "41%");
}

#[test]
fn codexs_ring_leads_with_its_session_when_it_reports_one() {
    let windows = vec![
        window(
            "secondary",
            "Weekly · all models",
            LimitWindowKind::Weekly,
            10.0,
        ),
        window(
            "primary",
            "5 hour · all models",
            LimitWindowKind::Session,
            20.0,
        ),
    ];
    assert_eq!(ring_label(AgentId::Codex, windows.clone()), "20%");
    assert_eq!(ring_label(AgentId::Codex, windows[..1].to_vec()), "10%");
}
#[test]
fn unreadable_providers_fall_back_to_the_dash_label() {
    let mut provider = provider_data(AgentId::Cursor, 50.0);
    provider.status = LimitsStatus::Failed;
    provider.message = Some("Could not read usage.".into());
    let content = cell_content(&CellData::Provider(provider));
    match content {
        CellContent::Provider { label, primary, .. } => {
            assert_eq!(label, "—");
            assert!(primary.is_none());
        }
        _ => panic!("wrong content kind"),
    }
}
#[test]
fn reset_notes_never_double_space_the_hour() {
    // One reset per hour of the day, so every timezone meets a single-digit hour.
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
        CellContent::Provider { label, primary, .. } => {
            assert_eq!(label, "0%", "the quota renewed at the reset");
            assert_eq!(
                primary.and_then(|quota| quota.percent),
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
    // The copy affordance sits at the row's right edge; a title line drawn semibold
    // has to stop before it, so the wrap must measure the weight it draws in.
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
    // Every run the mac draws `.medium` or `.semibold` has to carry that weight here
    // too. What the two resolve to is the font's business — Segoe UI ships no 500, so
    // DirectWrite lands both on semibold, exactly as it does for the app's own
    // `font-medium` — but the popover still has to ask for the right one.
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
    // Every run the mac draws `.medium`, and the ones it really draws `.semibold`.
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
    // And a medium run really is heavier ink than a regular one on this system. Segoe UI
    // has no 500, so DirectWrite resolves both medium and semibold onto the semibold cut
    // — the same thing it does for the app's own `font-medium`, which is why the two
    // measure alike here.
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

/// Every text entry of one open provider popover, in order.
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

fn inner_ring(provider: ProviderData) -> Option<InnerRing> {
    match cell_content(&CellData::Provider(provider)) {
        CellContent::Provider { inner, .. } => inner,
        _ => panic!("wrong content kind"),
    }
}

#[test]
fn a_codex_members_credit_share_fills_the_inner_ring_under_the_weekly() {
    let member = codex_member(share("8000", 32.0, false, "2099-01-01T12:00:00Z"));
    match cell_content(&CellData::Provider(member.clone())) {
        CellContent::Provider { label, primary, .. } => {
            assert_eq!(label, "31%", "the weekly stays the headline");
            assert_eq!(primary.and_then(|quota| quota.percent), Some(31.0));
        }
        _ => panic!("wrong content kind"),
    }
    assert_eq!(
        inner_ring(member),
        Some(InnerRing {
            quota: QuotaView {
                percent: Some(32.0),
                reached: false
            },
            ink: CREDITS_INK,
            track: CREDITS_TRACK,
        })
    );
    assert_eq!(
        inner_ring(codex_member(share(
            "25000",
            100.0,
            true,
            "2020-01-01T12:00:00Z"
        )))
        .map(|ring| ring.quota),
        Some(QuotaView {
            percent: Some(0.0),
            reached: false
        }),
        "a share past its reset has renewed, as a window has"
    );
    let mut unreadable = codex_member(share("8000", 32.0, false, "2099-01-01T12:00:00Z"));
    unreadable.status = LimitsStatus::Failed;
    assert!(inner_ring(unreadable).is_none());
}

/// Claude's Fable window takes the same inner ring, in its own terracotta.
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
    let ring = inner_ring(claude).expect("an inner ring");
    assert_eq!(ring.quota.percent, Some(13.0));
    assert_eq!((ring.ink, ring.track), (FABLE_ORANGE, FABLE_TRACK));
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
    // The date is the viewer's, built from the same instant in local time.
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

/// The popover words the share as the app does: a reached share with some left says only that the
/// limit is reached, and a renewed one says all of it is left.
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

/// A paused account with only a remembered share still says the values below are last observed.
#[test]
fn a_paused_account_with_only_a_share_says_its_values_are_last_observed() {
    let mut paused = codex_member(share("8000", 32.0, false, "2099-01-01T12:00:00Z"));
    paused.status = LimitsStatus::Failed;
    paused.message = Some("Refresh failed.".into());
    paused.windows.clear();

    let texts = popover_texts(paused);
    assert!(
        texts.contains(&"Refresh paused. Last observed values below.".to_string()),
        "{texts:?}"
    );
}
