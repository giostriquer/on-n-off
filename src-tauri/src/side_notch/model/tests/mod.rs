use super::*;

mod projection;
mod rail;

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
        providers: RAIL_ORDER.to_vec(),
        pull_requests: NotchPullRequests {
            enabled: false,
            lists: vec![GithubList::Mine],
        },
        ..NotchSettings::default()
    }
}

#[test]
fn muse_follows_codex_even_when_other_integrations_are_selected() {
    let mut selected = settings("main", Edge::Right);
    selected.providers = vec![
        AgentId::Cursor,
        AgentId::Muse,
        AgentId::Antigravity,
        AgentId::Codex,
        AgentId::Claude,
    ];
    assert_eq!(selected.rail_providers(), RAIL_ORDER);
    selected.providers = vec![AgentId::Cursor, AgentId::Claude, AgentId::Claude];
    assert_eq!(
        selected.rail_providers(),
        [AgentId::Claude, AgentId::Cursor]
    );
}

#[test]
fn the_pull_request_cell_is_on_by_default_with_only_the_users_own_list() {
    let defaults = NotchSettings::default();
    assert!(defaults.pull_requests.enabled);
    assert_eq!(defaults.pull_requests.lists, [GithubList::Mine]);
    assert_eq!(
        frame::cell_count(&defaults),
        4,
        "three providers plus the cell"
    );
    let legacy: NotchSettings =
        serde_json::from_str(r#"{"enabled":true,"displayId":"main","edge":"right"}"#).unwrap();
    assert_eq!(
        frame::cell_count(&legacy),
        6,
        "older documents gain the cell and keep their providers"
    );
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
        layout(&current, &displays, frame::cell_count(&current))
            .unwrap()
            .height,
        rail_metrics(NotchSize::Standard, 1.0, Edge::Right).length(6)
    );
    let json = serde_json::to_value(&current).unwrap();
    assert_eq!(json["pullRequests"]["enabled"], true);
    assert_eq!(json["pullRequests"]["lists"][0], "assigned");
}

#[test]
fn the_document_the_ui_checks_its_notch_settings_against_is_the_one_rust_writes() {
    let settings = NotchSettings {
        enabled: true,
        display_id: Some("display-1".into()),
        edge: Edge::Left,
        size: NotchSize::Large,
        show: ShowMode::OnHover,
        providers: vec![AgentId::Claude, AgentId::Codex],
        pull_requests: NotchPullRequests {
            enabled: false,
            lists: vec![GithubList::Mine, GithubList::Assigned],
        },
        popover_style: NotchStyle::Solid,
    };
    let fixture = include_str!("../fixtures/notch-settings.json");
    let written = serde_json::to_value(&settings).unwrap();

    assert_eq!(
        written,
        serde_json::from_str::<serde_json::Value>(fixture).unwrap(),
        "src-tauri/src/side_notch/model/fixtures/notch-settings.json must read:\n{}",
        serde_json::to_string_pretty(&written).unwrap()
    );
    assert_eq!(
        serde_json::from_str::<NotchSettings>(fixture).unwrap(),
        settings
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
fn notch_style_defaults_to_liquid_glass_and_saves_a_solid_choice() {
    let legacy: NotchSettings =
        serde_json::from_str(r#"{"enabled":true,"displayId":"main","edge":"right"}"#).unwrap();
    assert_eq!(legacy.popover_style, NotchStyle::Glass);
    assert_eq!(
        serde_json::to_value(&legacy).unwrap()["popoverStyle"],
        "glass"
    );
    let solid: NotchSettings = serde_json::from_str(
        r#"{"enabled":true,"displayId":"main","edge":"right","popoverStyle":"solid"}"#,
    )
    .unwrap();
    assert_eq!(solid.popover_style, NotchStyle::Solid);
    assert_eq!(
        serde_json::to_value(&solid).unwrap()["popoverStyle"],
        "solid"
    );
}

#[test]
fn liquid_glass_starts_at_macos_26() {
    assert!(!liquid_glass_on(25));
    assert!(liquid_glass_on(26));
}

#[cfg(target_os = "macos")]
#[test]
fn the_snapshot_reports_liquid_glass_for_the_running_macos() {
    let output = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .unwrap();
    let version = String::from_utf8(output.stdout).unwrap();
    let major: isize = version.trim().split('.').next().unwrap().parse().unwrap();
    assert_eq!(super::super::liquid_glass(), liquid_glass_on(major));
}

#[test]
fn a_fresh_notch_only_rails_the_providers_with_limits_to_show() {
    let fresh = NotchSettings::default();
    assert_eq!(
        fresh.providers,
        crate::provider_catalogue::WITH_LIMITS,
        "the ones with quotas to draw"
    );
    let chosen = NotchSettings {
        providers: RAIL_ORDER.to_vec(),
        ..NotchSettings::default()
    };
    assert_eq!(
        chosen.rail_providers(),
        RAIL_ORDER,
        "an explicit choice stands"
    );
}

#[test]
fn each_meter_level_draws_in_one_ink() {
    let base: Color = [217, 119, 87, 255];

    assert_eq!(meter_color(Meter::Unreadable, base), UNREADABLE_INK);
    assert_eq!(meter_color(Meter::Base, base), base);
    assert_eq!(meter_color(Meter::Trip, base), TRIP_RED);
    assert_eq!(meter_color(Meter::Blend(0.0), base), base);
    assert_eq!(meter_color(Meter::Blend(0.5), base), [222, 104, 82, 255]);
    assert_eq!(meter_color(Meter::Blend(1.0), base), TRIP_RED);
}

fn share(limit: &str, used: &str, used_percent: f64, reached: bool) -> LimitsWorkspaceCreditsDto {
    LimitsWorkspaceCreditsDto {
        limit: limit.into(),
        used: used.into(),
        used_percent,
        resets_at: Some("2026-10-01T12:00:00Z".into()),
        reached,
    }
}

#[test]
fn a_share_is_worded_once_for_both_notches() {
    let worded = |limit, used, reached| workspace_share_wording(&share(limit, used, 0.0, reached));

    assert_eq!(
        worded("25000", "8000", false),
        ShareWording {
            left: "17,000 of 25,000 left".into(),
            renewed: "25,000 of 25,000 left".into(),
        }
    );
    assert_eq!(worded("100", "120", false).left, "0 of 100 left");
    assert_eq!(
        worded("25000.5", "8000.25", false).left,
        "17,000.25 of 25,000.5 left"
    );
    assert_eq!(worded("10.125", "0", false).left, "10.13 of 10.13 left");
}

#[test]
fn a_reached_share_says_all_used_only_when_its_amounts_agree() {
    let left = |limit, used| workspace_share_wording(&share(limit, used, 100.0, true)).left;

    assert_eq!(left("10000", "10000"), "all 10,000 used");
    assert_eq!(left("100", "120"), "all 100 used");
    assert_eq!(left("25000", "24000"), "limit reached");
}

#[test]
fn an_organizations_pull_request_is_work_and_a_users_is_personal() {
    let pr = |in_organization: serde_json::Value| -> crate::dto::GithubPrDto {
        serde_json::from_value(serde_json::json!({
            "id": "node-1", "number": 1, "title": "Fix", "url": "https://github.com/acme/webapp/pull/1",
            "repo": "acme/webapp", "author": "octocat", "isDraft": false, "ci": "success",
            "headRef": "fix", "baseRef": "main", "updatedAt": "", "inOrganization": in_organization
        }))
        .unwrap()
    };
    assert_eq!(PrContext::of(&pr(true.into())), Some(PrContext::Work));
    assert_eq!(PrContext::of(&pr(false.into())), Some(PrContext::Personal));
    assert_eq!(
        PrContext::of(&pr(serde_json::Value::Null)),
        None,
        "a pull request read before GitHub reported its owner is neither"
    );
}
