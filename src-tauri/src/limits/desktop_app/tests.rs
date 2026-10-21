use std::path::PathBuf;

use super::*;
use crate::accounts::model::Identity;
use crate::accounts::{roster_for_test, SignInDeadlines};
use crate::dto::ProviderLimitsDto;
use crate::limits::{column, scoped_account};
use crate::paths;

fn identity(user: &str, organization: &str) -> Identity {
    Identity {
        provider: AgentId::Claude,
        user_id: user.into(),
        workspace_id: organization.into(),
    }
}

fn home_with(config: Option<&str>, installed: bool) -> PathBuf {
    let home = paths::scratch_dir("desktop-app");
    if let Some(config) = config {
        let data = paths::claude_desktop_data_for(&home);
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("config.json"), config).unwrap();
    }
    if installed {
        for app in paths::claude_desktop_apps_for(&home) {
            std::fs::create_dir_all(app).unwrap();
        }
    }
    home
}

fn config_naming(user: &str) -> String {
    serde_json::json!({
        "locale": "en-US",
        "lastKnownAccountUuid": user,
        "oauth:tokenCache": "encrypted-placeholder",
    })
    .to_string()
}

fn desktop_on(user: &str) -> PathBuf {
    home_with(Some(&config_naming(user)), true)
}

fn signed_in(identity: &Identity, label: &str) -> ProviderLimitsDto {
    let mut card = ProviderLimitsDto::for_test(identity.provider, "replaced");
    card.account = Some(scoped_account(identity, Some(label.to_owned())));
    card
}

fn saved(identity: &Identity, label: &str) -> ProviderLimitsDto {
    let mut card = signed_in(identity, label);
    card.current_account = false;
    card.saved_profile = true;
    card
}

fn column_of(
    provider: AgentId,
    entries: &[ProviderLimitsDto],
    profiles: &[(Identity, &str)],
) -> Vec<ColumnEntryDto> {
    column(
        entries,
        &roster_for_test(provider, profiles, &[], &[]),
        &SignInDeadlines::new(),
        0,
    )
}

fn marked_for(
    desktop: &str,
    entries: &[ProviderLimitsDto],
    profiles: &[(Identity, &str)],
) -> Vec<ColumnEntryDto> {
    let mut column = column_of(AgentId::Claude, entries, profiles);
    mark(DesktopApp::Claude, &mut column, desktop);
    column
}

fn tags(column: &[ColumnEntryDto]) -> Vec<bool> {
    column.iter().map(|entry| entry.desktop_account).collect()
}

#[test]
fn reads_the_account_an_installed_claude_desktop_last_signed_in_to() {
    assert_eq!(
        desktop_user(DesktopApp::Claude, &desktop_on("desktop-user")),
        Some("desktop-user".to_owned())
    );
}

#[test]
fn a_config_left_behind_by_an_uninstalled_desktop_names_no_account() {
    let left_behind = home_with(Some(&config_naming("desktop-user")), false);
    assert_eq!(desktop_user(DesktopApp::Claude, &left_behind), None);
    let never_signed_in = home_with(None, true);
    assert_eq!(desktop_user(DesktopApp::Claude, &never_signed_in), None);
}

#[test]
fn names_no_desktop_account_without_a_config_that_names_one() {
    for config in [
        "{",
        "{}",
        r#"{"lastKnownAccountUuid": null}"#,
        r#"{"lastKnownAccountUuid": "  "}"#,
        r#"{"lastKnownAccountUuid": 7}"#,
        "[]",
    ] {
        assert_eq!(
            desktop_user(DesktopApp::Claude, &home_with(Some(config), true)),
            None,
            "{config}"
        );
    }
}

#[test]
fn the_saved_account_claude_desktop_is_on_carries_the_tag() {
    let me = identity("user-me", "org-me");
    let desk = identity("user-desk", "org-desk");
    let column = marked_for(
        "user-desk",
        &[signed_in(&me, "me@claude.example")],
        &[(me, "me@claude.example"), (desk, "desk@claude.example")],
    );
    assert_eq!(tags(&column), [false, true]);
}

#[test]
fn the_read_card_claude_desktop_is_on_carries_the_tag() {
    let me = identity("user-me", "org-me");
    let desk = identity("user-desk", "org-desk");
    let column = marked_for(
        "user-desk",
        &[
            signed_in(&me, "me@claude.example"),
            saved(&desk, "desk@claude.example"),
        ],
        &[],
    );
    assert_eq!(tags(&column), [false, true]);
}

#[test]
fn every_card_of_the_user_claude_desktop_is_on_carries_the_tag() {
    let me = identity("user-me", "org-me");
    let column = marked_for(
        "user-desk",
        &[
            signed_in(&me, "me@claude.example"),
            saved(&identity("user-desk", "org-desk"), "desk@claude.example"),
            saved(&identity("user-desk", "org-team"), "desk@claude.example"),
        ],
        &[],
    );
    assert_eq!(tags(&column), [false, true, true]);
}

#[test]
fn an_account_no_card_shows_tags_nothing() {
    let me = identity("user-me", "org-me");
    let column = marked_for(
        "user-stranger",
        &[signed_in(&me, "me@claude.example")],
        &[(me, "me@claude.example")],
    );
    assert_eq!(tags(&column), [false]);
}

#[test]
fn no_tag_while_claude_desktop_is_on_the_signed_in_account() {
    let me = identity("user-me", "org-me");
    let scoped = marked_for("user-me", &[signed_in(&me, "me@claude.example")], &[]);
    assert_eq!(tags(&scoped), [false]);
    let saved_too = marked_for(
        "user-me",
        &[signed_in(&me, "me@claude.example")],
        &[(me.clone(), "me@claude.example")],
    );
    assert_eq!(tags(&saved_too), [false]);
    let unscoped = marked_for(
        "user-me",
        &[ProviderLimitsDto::for_test(AgentId::Claude, "user-me")],
        &[],
    );
    assert_eq!(tags(&unscoped), [false]);
    let other_organization = marked_for(
        "user-me",
        &[
            signed_in(&identity("user-me", "org-team"), "me@claude.example"),
            saved(&me, "me@claude.example"),
        ],
        &[],
    );
    assert_eq!(tags(&other_organization), [false, false]);
}

#[test]
fn an_unscoped_signed_in_card_is_compared_by_its_own_id() {
    let desk = identity("user-desk", "org-desk");
    let column = marked_for(
        "user-desk",
        &[
            ProviderLimitsDto::for_test(AgentId::Claude, "user-me"),
            saved(&desk, "desk@claude.example"),
        ],
        &[],
    );
    assert_eq!(tags(&column), [false, true]);
}

#[test]
fn no_tag_without_a_signed_in_claude_code_account() {
    let desk = identity("user-desk", "org-desk");
    let default = marked_for(
        "user-desk",
        &[
            ProviderLimitsDto::for_test(AgentId::Claude, "default"),
            saved(&desk, "desk@claude.example"),
        ],
        &[],
    );
    assert_eq!(tags(&default), [false, false]);
    let mut signed_out = ProviderLimitsDto::for_test(AgentId::Claude, "default");
    signed_out.account = None;
    assert_eq!(
        tags(&marked_for(
            "user-desk",
            &[signed_out, saved(&desk, "desk@claude.example")],
            &[]
        )),
        [false, false]
    );
    let saved_only = marked_for("user-desk", &[saved(&desk, "desk@claude.example")], &[]);
    assert_eq!(tags(&saved_only), [false]);
}

#[test]
fn marking_reads_the_desktop_app_of_a_provider_that_has_one() {
    let me = identity("user-me", "org-me");
    let desk = identity("user-desk", "org-desk");
    let home = desktop_on("user-desk");
    let mut claude = column_of(
        AgentId::Claude,
        &[
            signed_in(&me, "me@claude.example"),
            saved(&desk, "desk@claude.example"),
        ],
        &[],
    );
    mark_desktop_account(AgentId::Claude, &mut claude, &home);
    assert_eq!(tags(&claude), [false, true]);
    let codex = |identity: &Identity| Identity {
        provider: AgentId::Codex,
        ..identity.clone()
    };
    let mut column = column_of(
        AgentId::Codex,
        &[
            signed_in(&codex(&me), "me@codex.example"),
            saved(&codex(&desk), "desk@codex.example"),
        ],
        &[],
    );
    mark_desktop_account(AgentId::Codex, &mut column, &home);
    assert_eq!(tags(&column), [false, false]);
}

#[test]
fn the_tag_reaches_the_ui_only_on_the_marked_card() {
    let me = identity("user-me", "org-me");
    let desk = identity("user-desk", "org-desk");
    let column = marked_for(
        "user-desk",
        &[signed_in(&me, "me@claude.example")],
        &[(desk, "desk@claude.example")],
    );
    let sent: Vec<serde_json::Value> = column
        .iter()
        .map(|entry| serde_json::to_value(entry).unwrap())
        .collect();
    assert!(sent[0].get("desktopAccount").is_none());
    assert_eq!(sent[1]["desktopAccount"], serde_json::json!(true));
}

#[test]
#[ignore = "reads Claude Desktop's and Claude Code's account in the home ON_N_OFF_PROBE_HOME names"]
fn probe_real_home_desktop_account() {
    let home = paths::probe_home();
    let desktop = last_account(DesktopApp::Claude, &home);
    let code = crate::limits::claude_config::read_claude_config_account(&home.join(".claude.json"))
        .map(|account| account.identity.account.id);
    println!("Claude Desktop account read: {}", desktop.is_some());
    println!("Claude Code account read: {}", code.is_some());
    println!("same account: {}", desktop.is_some() && desktop == code);
}
