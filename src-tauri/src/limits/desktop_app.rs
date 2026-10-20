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

pub(crate) fn mark_other_account(
    provider: AgentId,
    column: &mut [ColumnEntryDto],
    home: Option<&Path>,
) {
    let Some(app) = provider.limits_facts().desktop_app else {
        return;
    };
    let Some(desktop) = home.and_then(|home| signed_in_user(app, home)) else {
        return;
    };
    let Some(signed_in) = column.iter().position(|entry| entry.card.current_account) else {
        return;
    };
    if user_of(app, &column[signed_in]).is_none_or(|user| user == desktop) {
        return;
    }
    let label = column
        .iter()
        .filter(|entry| user_of(app, entry) == Some(desktop.as_str()))
        .find_map(label_of);
    column[signed_in].other_desktop_account = Some(OtherDesktopAccountDto { label });
}

pub(crate) fn signed_in_user(app: DesktopApp, home: &Path) -> Option<String> {
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

fn label_of(entry: &ColumnEntryDto) -> Option<String> {
    entry.card.account.as_ref()?.label.clone()
}

#[cfg(test)]
mod tests;
