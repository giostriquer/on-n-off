use std::path::Path;

use serde::Deserialize;

use super::DEFAULT_ACCOUNT;
use crate::accounts::model::Identity;
use crate::dto::{AgentId, ColumnEntryDto, OtherDesktopAccountDto};
use crate::paths;
use crate::provider_catalogue::DesktopApp;

#[derive(Deserialize)]
struct ClaudeDesktopConfig {
    #[serde(rename = "lastKnownAccountUuid")]
    last_known_account: Option<String>,
}

pub(crate) fn mark_other_account(provider: AgentId, column: &mut [ColumnEntryDto], home: &Path) {
    let Some(app) = provider.limits_facts().desktop_app else {
        return;
    };
    if signed_in(app, column).is_none() {
        return;
    }
    if let Some(desktop) = desktop_user(app, home) {
        mark(app, column, &desktop);
    }
}

fn mark(app: DesktopApp, column: &mut [ColumnEntryDto], desktop: &str) {
    let Some((index, user)) = signed_in(app, column) else {
        return;
    };
    if user == desktop {
        return;
    }
    let label = column
        .iter()
        .filter(|entry| user_of(app, entry) == Some(desktop))
        .find_map(|entry| entry.card.account.as_ref()?.label.clone());
    column[index].other_desktop_account = Some(OtherDesktopAccountDto { label });
}

fn signed_in(app: DesktopApp, column: &[ColumnEntryDto]) -> Option<(usize, &str)> {
    let index = column.iter().position(|entry| entry.card.current_account)?;
    Some((index, user_of(app, &column[index])?))
}

fn desktop_user(app: DesktopApp, home: &Path) -> Option<String> {
    if !installed(app, home) {
        return None;
    }
    last_account(app, home)
}

fn installed(app: DesktopApp, home: &Path) -> bool {
    match app {
        DesktopApp::Claude => paths::claude_desktop_apps_for(home)
            .iter()
            .any(|app| app.exists()),
    }
}

fn last_account(app: DesktopApp, home: &Path) -> Option<String> {
    match app {
        DesktopApp::Claude => {
            let raw =
                std::fs::read(paths::claude_desktop_data_for(home).join("config.json")).ok()?;
            serde_json::from_slice::<ClaudeDesktopConfig>(&raw)
                .ok()?
                .last_known_account
                .filter(|id| !id.trim().is_empty())
        }
    }
}

fn user_of(app: DesktopApp, entry: &ColumnEntryDto) -> Option<&str> {
    match app {
        DesktopApp::Claude => {
            let account = entry.card.account.as_ref()?;
            if Identity::is_profile_key(&account.id) {
                account.legacy_id.as_deref()
            } else {
                Some(account.id.as_str()).filter(|id| *id != DEFAULT_ACCOUNT)
            }
        }
    }
}

#[cfg(test)]
mod tests;
