use super::*;
use crate::side_notch::model::{GithubList, NotchSize};
use crate::side_notch::sessions::SessionStatus;

fn rect(r: &R) -> String {
    format!("{} {} {} {}", r.x, r.y, r.w, r.h)
}

fn ink(color: &Color) -> String {
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        color[0], color[1], color[2], color[3]
    )
}

fn content(content: &CellContent) -> String {
    match content {
        CellContent::Provider {
            provider,
            headline,
            inner,
            label,
        } => format!(
            "{provider:?} {label} headline {:?} inner {:?}",
            headline.and_then(|at| at.used_now()),
            inner
                .as_ref()
                .map(|(ring, at)| (ring.clone(), at.used_now()))
        ),
        CellContent::PullRequests { label, segments } => {
            format!("pull requests {label} segments {}", segments.len())
        }
    }
}

fn item(item: &PopItem) -> String {
    match item {
        PopItem::Mark { kind, size } => format!("mark {kind:?} {size}"),
        PopItem::Text {
            text,
            size,
            weight,
            color,
            right,
        } => format!(
            "text {text:?} {size} {weight:?} {} {}",
            ink(color),
            if *right { "right" } else { "left" }
        ),
        PopItem::Bar { percent, color } => format!("bar {percent:?} {}", ink(color)),
        PopItem::Divider => "divider".into(),
        PopItem::Dot { ci } => format!("dot {ci:?}"),
        PopItem::CopyIcon { color } => format!("copy {}", ink(color)),
        PopItem::Stripe { color } => format!("stripe {}", ink(color)),
    }
}

fn describe(plan: &Plan) -> Vec<String> {
    let mut lines = vec![
        format!(
            "{:?} display {} size {}",
            plan.edge, plan.display_scale, plan.size_scale
        ),
        format!("window {}", rect(&plan.window)),
        format!("rail {}", rect(&plan.rail)),
        format!("ear {}", plan.metrics.ear()),
    ];
    for (index, cell) in plan.cells.iter().enumerate() {
        lines.push(format!(
            "cell {index} {}{} {}",
            rect(&cell.rect),
            if cell.active { " active" } else { "" },
            content(&cell.content)
        ));
    }
    lines.push(format!(
        "cap {} hovered {} pinned {}",
        rect(&plan.cap.rect),
        plan.cap.hovered,
        plan.cap.pinned
    ));
    if let Some(pill) = &plan.pill {
        lines.push(format!("pill {}", rect(pill)));
    }
    if let Some(popover) = &plan.popover {
        lines.push(format!(
            "popover {} card {} tail {}",
            rect(&popover.rect),
            rect(&popover.card),
            popover.tail
        ));
        for (entry, at) in &popover.entries {
            lines.push(format!("  {} @ {}", item(entry), rect(at)));
        }
        for (zone, at) in &popover.zones {
            lines.push(format!("  zone {zone:?} @ {}", rect(at)));
        }
        if let Some(error) = &popover.action_error {
            lines.push(format!("  action error {error:?}"));
        }
    }
    lines
}

fn unknown_reset(id: &str, label: &str, kind: LimitWindowKind, percent: f64) -> LimitWindowDto {
    LimitWindowDto {
        resets_at: None,
        ..window(id, label, kind, percent)
    }
}

fn session(name: &str, status: SessionStatus, minutes_ago: i64) -> LiveSession {
    LiveSession {
        id: name.into(),
        name: name.into(),
        place: "Terminal".into(),
        project: "webapp".into(),
        status,
        last_active_at: (now() - chrono::Duration::minutes(minutes_ago)).to_rfc3339(),
    }
}

fn card(provider: AgentId, reading: Reading) -> ProviderLimitsDto {
    crate::limits::signed_in_card(provider, "acct", reading)
}

fn failed(mut card: ProviderLimitsDto, message: &str) -> ProviderLimitsDto {
    card.status = LimitsStatus::Failed;
    card.message = Some(message.into());
    card
}

fn pulls_dto(status: &str, hint: Option<&str>, stale: bool) -> GithubPrsDto {
    let mut long = pr(
        42,
        "Fix the flaky resize test that fails on every second Windows build",
        "acme/webapp",
    );
    long["mergeKind"] = "behind".into();
    long["isDraft"] = true.into();
    long["inOrganization"] = true.into();
    let mut failing = pr(7, "Bump the toolchain", "octocat/dotfiles");
    failing["ci"] = "failure".into();
    failing["reviewDecision"] = "CHANGES_REQUESTED".into();
    failing["inOrganization"] = false.into();
    let mut pending = pr(
        1234,
        "Name the plan",
        "octocat/a-repository-name-long-enough-to-run-into-the-ci-dot",
    );
    pending["ci"] = "pending".into();
    pending["reviewDecision"] = "APPROVED".into();
    pending["mergeKind"] = "ready".into();
    serde_json::from_value(serde_json::json!({
        "status": status, "hint": hint, "stale": stale, "scope": [],
        "mine": {"total": 5, "items": [long, failing, pending]},
        "reviewRequested": {"total": 0, "items": []},
        "assigned": {"total": 0, "items": []}
    }))
    .expect("a pull request read")
}

struct Case {
    name: &'static str,
    settings: NotchSettings,
    display: Display,
    data: RailData,
    hover: Hover,
}

fn rail_settings(edge: Edge, size: NotchSize, providers: Vec<AgentId>) -> NotchSettings {
    NotchSettings {
        edge,
        size,
        providers,
        pull_requests: NotchPullRequests {
            enabled: true,
            lists: vec![GithubList::Mine, GithubList::ReviewRequested],
        },
        ..settings()
    }
}

fn opened(index: usize) -> Hover {
    Hover {
        active: Some(index),
        ..Hover::default()
    }
}

fn cases() -> Vec<Case> {
    let claude = NotchProvider::current(vec![card(
        AgentId::Claude,
        Reading {
            windows: vec![
                unknown_reset(
                    "weekly",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    41.0,
                ),
                unknown_reset(
                    "session",
                    "5 hour · all models",
                    LimitWindowKind::Session,
                    73.0,
                ),
                LimitWindowDto {
                    role: Some(crate::dto::WindowRole::InnerRing),
                    ..unknown_reset("fable", "Weekly · Fable", LimitWindowKind::Model, 13.0)
                },
            ],
            ..Reading::default()
        },
    )]);
    let sessions = vec![
        session("notch", SessionStatus::Working, 0),
        session(
            "a-very-long-running-session-name-that-cannot-possibly-fit",
            SessionStatus::Idle,
            95,
        ),
    ];
    let paused_codex = NotchProvider::current(vec![failed(
        card(
            AgentId::Codex,
            Reading {
                windows: vec![unknown_reset(
                    "weekly",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    88.0,
                )],
                ..Reading::default()
            },
        ),
        "Refresh failed.",
    )]);
    let unavailable = NotchProvider::current(vec![failed(
        card(AgentId::Claude, Reading::default()),
        "Claude Code reported no usage, and this message runs long enough to wrap onto more lines.",
    )]);
    let member = NotchProvider::current(vec![card(
        AgentId::Codex,
        Reading {
            windows: vec![unknown_reset(
                "primary",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                31.0,
            )],
            workspace_credits: Some(LimitsWorkspaceCreditsDto {
                limit: "25000".into(),
                used: "8000".into(),
                used_percent: 32.0,
                resets_at: None,
                reached: false,
            }),
            ..Reading::default()
        },
    )]);

    let mut cases = Vec::new();
    let both = |edge, size| rail_settings(edge, size, vec![AgentId::Claude, AgentId::Codex]);

    let live = both(Edge::Right, NotchSize::Standard);
    cases.push(Case {
        name: "a live provider with sessions on the right at 1x",
        data: frame::rail(
            &live,
            [
                (AgentId::Claude, claude.clone(), sessions.clone()),
                (AgentId::Codex, member.clone(), Vec::new()),
            ],
            Some(&pulls_dto("ok", None, false)),
            None,
        ),
        settings: live,
        display: display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0),
        hover: opened(0),
    });

    let paused = both(Edge::Top, NotchSize::Compact);
    cases.push(Case {
        name: "a paused provider on the top at compact 1.5x",
        data: frame::rail(
            &paused,
            [
                (AgentId::Claude, claude.clone(), Vec::new()),
                (AgentId::Codex, paused_codex, Vec::new()),
            ],
            None,
            None,
        ),
        settings: paused,
        display: display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.5),
        hover: opened(1),
    });

    let left = both(Edge::Left, NotchSize::Large);
    cases.push(Case {
        name: "an unavailable provider with an action error on the left at large 1.25x",
        data: frame::rail(
            &left,
            [(AgentId::Claude, unavailable, Vec::new())],
            None,
            Some("Could not open the pull request.".into()),
        ),
        settings: left,
        display: display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.25),
        hover: opened(0),
    });

    let bottom = both(Edge::Bottom, NotchSize::Standard);
    cases.push(Case {
        name: "a workspace share on the bottom at 2x",
        data: frame::rail(
            &bottom,
            [(AgentId::Codex, member.clone(), Vec::new())],
            None,
            None,
        ),
        settings: bottom.clone(),
        display: display("d1", 0.0, 0.0, 1920.0, 1080.0, 2.0),
        hover: opened(1),
    });
    cases.push(Case {
        name: "a provider not read yet on the bottom at 2x",
        data: frame::rail(&bottom, [(AgentId::Codex, member, Vec::new())], None, None),
        settings: bottom,
        display: display("d1", 0.0, 0.0, 1920.0, 1080.0, 2.0),
        hover: opened(0),
    });

    let pulls_right = both(Edge::Right, NotchSize::Standard);
    cases.push(Case {
        name: "pull requests in two lists on the right at 1x",
        data: frame::rail(
            &pulls_right,
            [(AgentId::Claude, claude.clone(), Vec::new())],
            Some(&pulls_dto("ok", None, false)),
            None,
        ),
        settings: pulls_right,
        display: display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0),
        hover: opened(2),
    });

    let stale = both(Edge::Top, NotchSize::Compact);
    cases.push(Case {
        name: "stale pull requests with their notice on the top at compact 1.5x",
        data: frame::rail(
            &stale,
            [(AgentId::Claude, claude.clone(), Vec::new())],
            Some(&pulls_dto(
                "rateLimited",
                Some("GitHub is rate limiting requests; showing the last read."),
                true,
            )),
            None,
        ),
        settings: stale,
        display: display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.5),
        hover: opened(2),
    });

    let unreadable = both(Edge::Bottom, NotchSize::Large);
    cases.push(Case {
        name: "unreadable pull requests with an action error on the bottom at large 1.25x",
        data: frame::rail(
            &unreadable,
            [(AgentId::Claude, claude.clone(), Vec::new())],
            Some(&pulls_dto(
                "ghNotLoggedIn",
                Some("Sign in with the GitHub CLI."),
                false,
            )),
            Some("Could not copy the review request.".into()),
        ),
        settings: unreadable,
        display: display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.25),
        hover: opened(2),
    });

    let mut collapsed = both(Edge::Left, NotchSize::Compact);
    collapsed.show = ShowMode::OnHover;
    cases.push(Case {
        name: "a collapsed rail on the left at compact 1x",
        data: frame::rail(
            &collapsed,
            [(AgentId::Claude, claude.clone(), Vec::new())],
            None,
            None,
        ),
        settings: collapsed,
        display: display("d1", 0.0, 0.0, 1920.0, 1080.0, 1.0),
        hover: opened(0),
    });

    let mut capped = both(Edge::Top, NotchSize::Large);
    capped.show = ShowMode::OnHover;
    cases.push(Case {
        name: "an open rail with its cap hovered on the top at large 2x",
        data: frame::rail(&capped, [(AgentId::Claude, claude, Vec::new())], None, None),
        settings: capped,
        display: display("d1", 0.0, 0.0, 1920.0, 1080.0, 2.0),
        hover: Hover {
            rail_open: true,
            cap_hovered: true,
            active: None,
        },
    });
    cases
}

#[test]
fn every_rect_of_the_plan_is_pinned() {
    let planned: serde_json::Map<String, serde_json::Value> = cases()
        .into_iter()
        .map(|case| {
            let plan = plan(
                &case.settings,
                std::slice::from_ref(&case.display),
                &case.data,
                case.hover,
                now(),
            )
            .unwrap_or_else(|| panic!("{} plans", case.name));
            (case.name.to_string(), describe(&plan).into())
        })
        .collect();
    let planned = serde_json::Value::Object(planned);
    assert_eq!(
        planned,
        serde_json::from_str::<serde_json::Value>(include_str!("../fixtures/plans.json")).unwrap(),
        "src-tauri/src/side_notch/plan/fixtures/plans.json must read:\n{}",
        serde_json::to_string_pretty(&planned).unwrap()
    );
}
