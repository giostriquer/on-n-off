use super::*;

#[test]
fn codex_hides_internal_windows_from_both_ring_and_popover() {
    let provider = ProviderData {
        provider: AgentId::Codex,
        status: LimitsStatus::Ok,
        message: None,
        windows: vec![
            LimitWindowDto {
                id: "w-session".into(),
                label: "Session".into(),
                kind: LimitWindowKind::Session,
                used_percent: 10.0,
                resets_at: None,
                window_seconds: None,
                observed_at: "2026-09-01T10:00:00Z".into(),
            },
            LimitWindowDto {
                id: "extra:base_model_inference".into(),
                label: "Weekly · gpt-5.3-codex-spark".into(),
                kind: LimitWindowKind::Model,
                used_percent: 90.0,
                resets_at: None,
                window_seconds: None,
                observed_at: "2026-09-01T10:00:00Z".into(),
            },
        ],
        sessions: Vec::new(),
    };
    let displays = vec![display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0)];
    let planned = plan(
        &settings(),
        &displays,
        &data(vec![CellData::Provider(provider)]),
        Hover {
            active: Some(0),
            ..Hover::default()
        },
    )
    .unwrap();
    let popover = planned.popover.unwrap();
    let bars = popover
        .entries
        .iter()
        .filter(|(item, _)| matches!(item, PopItem::Bar { .. }))
        .count();
    assert_eq!(
        bars, 1,
        "the internal codex window never reaches the popover"
    );
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
fn a_reset_window_shows_a_dash_instead_of_a_stale_figure() {
    let mut expired = window("w", "Weekly - all models", LimitWindowKind::Weekly, 97.0);
    expired.resets_at = Some("2020-01-01T00:00:00Z".into());
    let content = cell_content(&CellData::Provider(claude_with(vec![expired.clone()])));
    match content {
        CellContent::Provider { label, primary, .. } => {
            assert_eq!(label, "—", "the figure is stale once the window reset");
            assert_eq!(
                primary.and_then(|quota| quota.percent),
                None,
                "and the ring draws no arc"
            );
        }
        _ => panic!("wrong content kind"),
    }

    let (planned, _) = popover_render(claude_with(vec![expired]));
    let popover = planned.popover.as_ref().expect("the popover is open");
    assert!(
        popover
            .entries
            .iter()
            .any(|(item, _)| matches!(item, PopItem::Text { text, .. } if text == "—")),
        "the popover says so too"
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
fn the_popover_keeps_the_mac_medium_weight_off_semibold() {
    // Segoe UI has no medium face, so the mac `.medium` runs land on the regular one;
    // semibold is a full step heavier — about a fifth wider — and reads chunky beside
    // the labels that really are semibold.
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
    // And the two weights really are different ink on this system.
    let regular = measure_weight("Open Limits", 11.0, TextWeight::Medium, 1.0);
    let heavy = measure_weight("Open Limits", 11.0, TextWeight::Semibold, 1.0);
    assert!(
        heavy > regular,
        "semibold is the heavier face: {regular} vs {heavy}"
    );
}
