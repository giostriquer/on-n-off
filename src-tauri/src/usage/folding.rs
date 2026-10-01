use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{SecondsFormat, TimeZone, Utc};

use crate::dto::{AdapterError, UsageHistoryState, UsageHistoryStatusDto};
use crate::paths::user_home;

use super::history::{
    clear_history, fold_cutoff_ms, fold_due, history_bytes, history_path_for, HistoryStore, DAY_MS,
    FOLD_AFTER_DAYS,
};
use super::sources::{lock_usage_files, Sources};

const FIRST_FOLD_DELAY: Duration = Duration::from_secs(90);
const FOLD_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

const UNREADABLE_GRACE_MS: i64 = FOLD_AFTER_DAYS * DAY_MS;

#[derive(Debug, Default)]
pub(crate) struct FoldChecks {
    unread_last_time: HashSet<String>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

pub fn spawn_history_folding() {
    let spawned = std::thread::Builder::new()
        .name("usage-history".into())
        .spawn(|| {
            std::thread::sleep(FIRST_FOLD_DELAY);
            let mut checks = FoldChecks::default();
            loop {
                let checked =
                    user_home().and_then(|home| fold_history_in(&home, now_ms(), &mut checks));
                if let Err(error) = checked {
                    eprintln!("usage history: {}", error.message);
                }
                std::thread::sleep(FOLD_CHECK_INTERVAL);
            }
        });
    if let Err(error) = spawned {
        eprintln!("usage history: could not start folding: {error}");
    }
}

pub fn usage_history_status() -> Result<UsageHistoryStatusDto, AdapterError> {
    Ok(history_status_in(&user_home()?))
}

pub fn clear_usage_history() -> Result<UsageHistoryStatusDto, AdapterError> {
    let home = user_home()?;
    clear_history_in(&home)?;
    Ok(history_status_in(&home))
}

pub(crate) fn fold_history_in(
    home: &Path,
    now_ms: i64,
    checks: &mut FoldChecks,
) -> Result<(), AdapterError> {
    let lock = lock_usage_files();
    let mut store = HistoryStore::open(history_path_for(home));
    if store.history().is_none() || !fold_due(store.watermark(), now_ms) {
        return Ok(());
    }
    let mut sources = Sources::open(lock, home, || store.watermark());

    let mut saved = Ok(());
    if sources.walked_every_root() {
        let cutoff_ms = fold_cutoff_ms(now_ms);
        let read = sources.read(i64::MIN, || store.watermark());
        let waiting = read.unread_files.iter().any(|(path, mtime_ms)| {
            *mtime_ms >= cutoff_ms - UNREADABLE_GRACE_MS || !checks.unread_last_time.contains(path)
        });
        checks.unread_last_time = read
            .unread_files
            .iter()
            .map(|(path, _)| path.clone())
            .collect();
        if !waiting {
            saved = store.fold(read.records(), cutoff_ms, now_ms);
        }
    }
    sources.finish(|| store.watermark());
    saved.map_err(|error| AdapterError::message(format!("Could not save a fold: {error}")))
}

pub(crate) fn history_status_in(home: &Path) -> UsageHistoryStatusDto {
    let path = history_path_for(home);
    let store = HistoryStore::open(path.clone());
    let iso = |ms: i64| {
        Utc.timestamp_millis_opt(ms)
            .single()
            .map(|at| at.to_rfc3339_opts(SecondsFormat::Millis, true))
    };
    let (state, kept_since, folded_through) = match store.history() {
        None => (UsageHistoryState::Unreadable, None, None),
        Some(history) => {
            let kept_since = history.kept_since_ms();
            let state = if kept_since.is_some() {
                UsageHistoryState::Kept
            } else {
                UsageHistoryState::Empty
            };
            (
                state,
                kept_since.and_then(iso),
                history.watermark().ms().and_then(iso),
            )
        }
    };
    UsageHistoryStatusDto {
        state,
        kept_since,
        folded_through,
        bytes: history_bytes(&path),
    }
}

pub(crate) fn clear_history_in(home: &Path) -> Result<(), AdapterError> {
    let _lock = lock_usage_files();
    clear_history(&history_path_for(home)).map_err(|error| {
        AdapterError::message(format!("Could not clear the usage history: {error}"))
    })
}

#[cfg(test)]
mod tests;
