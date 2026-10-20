use std::path::PathBuf;

use super::*;
use crate::accounts::model::Identity;
use crate::accounts::{roster_for_test, SignInDeadlines};
use crate::dto::ProviderLimitsDto;
use crate::limits::column;
use crate::paths;

fn identity(user: &str, organization: &str) -> Identity {
    Identity {
        provider: AgentId::Claude,
        user_id: user.into(),
        workspace_id: organization.into(),
    }
}

fn home_with_config(config: &str) -> PathBuf {
    let home = paths::scratch_dir("desktop-app");
    let data = paths::claude_desktop_data_for(&home);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("config.json"), config).unwrap();
    home
}

fn desktop_on(user: &str) -> PathBuf {
    home_with_config(
        &serde_json::json!({
            "locale": "en-US",
            "lastKnownAccountUuid": user,
            "oauth:tokenCache": "encrypted-placeholder",
        })
        .to_string(),
    )
}

fn signed_in(identity: &Identity, label: &str) -> ProviderLimitsDto {
    ProviderLimitsDto::for_test(identity.provider, &identity.observation_key())
        .labelled(label)
        .with_legacy_id(&identity.user_id)
}

fn saved(identity: &Identity, label: &str) -> ProviderLimitsDto {
    let mut card = signed_in(identity, label);
    card.current_account = false;
    card.saved_profile = true;
    card
}

fn marked(
    provider: AgentId,
    entries: &[ProviderLimitsDto],
    profiles: &[(Identity, &str)],
    home: Option<&Path>,
) -> Vec<ColumnEntryDto> {
    let mut column = column(
        entries,
        &roster_for_test(provider, profiles, &[], &[]),
        &SignInDeadlines::new(),
        0,
    );
    mark_other_account(provider, &mut column, home);
    column
}

fn tags(column: &[ColumnEntryDto]) -> Vec<Option<Option<&str>>> {
    column
        .iter()
        .map(|entry| {
            entry
                .other_desktop_account
                .as_ref()
                .map(|account| account.label.as_deref())
        })
        .collect()
}

#[test]
fn reads_the_account_claude_desktop_last_signed_in_to() {
    assert_eq!(
        signed_in_user(DesktopApp::Claude, &desktop_on("desktop-user")),
        Some("desktop-user".to_owned())
    );
}

#[test]
fn names_no_desktop_account_without_a_config_that_names_one() {
    let missing = paths::scratch_dir("desktop-app");
    assert_eq!(signed_in_user(DesktopApp::Claude, &missing), None);
    for config in [
        "{",
        "{}",
        r#"{"lastKnownAccountUuid": null}"#,
        r#"{"lastKnownAccountUuid": "  "}"#,
        r#"{"lastKnownAccountUuid": 7}"#,
        "[]",
    ] {
        assert_eq!(
            signed_in_user(DesktopApp::Claude, &home_with_config(config)),
            None,
            "{config}"
        );
    }
}

#[test]
fn the_signed_in_card_names_the_saved_account_claude_desktop_is_on() {
    let me = identity("user-me", "org-me");
    let desk = identity("user-desk", "org-desk");
    let home = desktop_on("user-desk");
    let column = marked(
        AgentId::Claude,
        &[signed_in(&me, "me@claude.example")],
        &[(me, "me@claude.example"), (desk, "desk@claude.example")],
        Some(&home),
    );
    assert_eq!(tags(&column), [Some(Some("desk@claude.example")), None]);
}

#[test]
fn a_read_card_names_the_account_claude_desktop_is_on() {
    let me = identity("user-me", "org-me");
    let desk = identity("user-desk", "org-desk");
    let home = desktop_on("user-desk");
    let column = marked(
        AgentId::Claude,
        &[
            signed_in(&me, "me@claude.example"),
            saved(&desk, "desk@claude.example"),
        ],
        &[],
        Some(&home),
    );
    assert_eq!(tags(&column), [Some(Some("desk@claude.example")), None]);
}

#[test]
fn an_account_no_card_shows_is_another_account() {
    let me = identity("user-me", "org-me");
    let home = desktop_on("user-stranger");
    let column = marked(
        AgentId::Claude,
        &[signed_in(&me, "me@claude.example")],
        &[(me, "me@claude.example")],
        Some(&home),
    );
    assert_eq!(tags(&column), [Some(None)]);
}

#[test]
fn no_tag_while_claude_desktop_is_on_the_signed_in_account() {
    let me = identity("user-me", "org-me");
    let home = desktop_on("user-me");
    let scoped = marked(
        AgentId::Claude,
        &[signed_in(&me, "me@claude.example")],
        &[],
        Some(&home),
    );
    assert_eq!(tags(&scoped), [None]);
    let saved = marked(
        AgentId::Claude,
        &[signed_in(&me, "me@claude.example")],
        &[(me.clone(), "me@claude.example")],
        Some(&home),
    );
    assert_eq!(tags(&saved), [None]);
    let unscoped = marked(
        AgentId::Claude,
        &[ProviderLimitsDto::for_test(AgentId::Claude, "user-me")],
        &[],
        Some(&home),
    );
    assert_eq!(tags(&unscoped), [None]);
    let other_organization = marked(
        AgentId::Claude,
        &[signed_in(
            &identity("user-me", "org-team"),
            "me@claude.example",
        )],
        &[],
        Some(&home),
    );
    assert_eq!(tags(&other_organization), [None]);
}

#[test]
fn an_unscoped_signed_in_card_is_compared_by_its_own_id() {
    let home = desktop_on("user-desk");
    let column = marked(
        AgentId::Claude,
        &[ProviderLimitsDto::for_test(AgentId::Claude, "user-me")],
        &[],
        Some(&home),
    );
    assert_eq!(tags(&column), [Some(None)]);
}

#[test]
fn no_tag_without_a_signed_in_claude_code_account() {
    let home = desktop_on("user-desk");
    let default = marked(
        AgentId::Claude,
        &[ProviderLimitsDto::for_test(AgentId::Claude, "default")],
        &[],
        Some(&home),
    );
    assert_eq!(tags(&default), [None]);
    let mut signed_out = ProviderLimitsDto::for_test(AgentId::Claude, "default");
    signed_out.account = None;
    assert_eq!(
        tags(&marked(AgentId::Claude, &[signed_out], &[], Some(&home))),
        [None]
    );
    let me = identity("user-me", "org-me");
    let saved_only = marked(
        AgentId::Claude,
        &[saved(&me, "me@claude.example")],
        &[],
        Some(&home),
    );
    assert_eq!(tags(&saved_only), [None]);
}

#[test]
fn no_tag_for_a_provider_without_a_desktop_app_or_without_a_home() {
    let home = desktop_on("user-desk");
    let codex = Identity {
        provider: AgentId::Codex,
        ..identity("user-me", "org-me")
    };
    assert_eq!(
        tags(&marked(
            AgentId::Codex,
            &[signed_in(&codex, "me@codex.example")],
            &[],
            Some(&home)
        )),
        [None]
    );
    let me = identity("user-me", "org-me");
    assert_eq!(
        tags(&marked(
            AgentId::Claude,
            &[signed_in(&me, "me@claude.example")],
            &[],
            None
        )),
        [None]
    );
}

#[test]
fn the_tag_reaches_the_ui_only_on_the_marked_card() {
    let me = identity("user-me", "org-me");
    let desk = identity("user-desk", "org-desk");
    let home = desktop_on("user-desk");
    let column = marked(
        AgentId::Claude,
        &[signed_in(&me, "me@claude.example")],
        &[(desk, "desk@claude.example")],
        Some(&home),
    );
    let sent: Vec<serde_json::Value> = column
        .iter()
        .map(|entry| serde_json::to_value(entry).unwrap())
        .collect();
    assert_eq!(
        sent[0]["otherDesktopAccount"],
        serde_json::json!({"label": "desk@claude.example"})
    );
    assert!(sent[1].get("otherDesktopAccount").is_none());
    let stranger = marked(
        AgentId::Claude,
        &[signed_in(&me, "me@claude.example")],
        &[],
        Some(&desktop_on("user-stranger")),
    );
    assert_eq!(
        serde_json::to_value(&stranger[0]).unwrap()["otherDesktopAccount"],
        serde_json::json!({"label": null})
    );
}

#[test]
#[ignore = "reads Claude Desktop's and Claude Code's account in the home ON_N_OFF_PROBE_HOME names"]
fn probe_real_home_desktop_account() {
    let home = paths::probe_home();
    let desktop = signed_in_user(DesktopApp::Claude, &home);
    let code = crate::limits::claude_config::read_claude_config_account(&home.join(".claude.json"))
        .map(|account| account.identity.account.id);
    println!("Claude Desktop account read: {}", desktop.is_some());
    println!("Claude Code account read: {}", code.is_some());
    println!("same account: {}", desktop.is_some() && desktop == code);
}
