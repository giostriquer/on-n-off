//! Folding usage into the history as it ages (`history`), off the Usage screen's path: a
//! background thread checks a little after launch and hourly after that, so usage is kept even
//! when the screen is never opened. Settings reads how far back the history reaches and can
//! clear it.

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

/// Out of the way of startup's own reads.
const FIRST_FOLD_DELAY: Duration = Duration::from_secs(90);
/// A fold is due once a day at most; a check that finds none reads only the history file.
const FOLD_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// A transcript that cannot be read holds a fold back while it may still be written to, or while
/// its failure may be passing (another process holding it). One last written this long before the
/// cutoff that failed on the previous check too never will read, and waiting on it would let the
/// provider delete every other transcript before it is kept.
const UNREADABLE_GRACE_MS: i64 = FOLD_AFTER_DAYS * DAY_MS;

/// What the background checks remember between runs: the transcripts the last one that got as far
/// as reading them could not read.
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

/// Clears the history and says what it holds now.
pub fn clear_usage_history() -> Result<UsageHistoryStatusDto, AdapterError> {
    let home = user_home()?;
    clear_history_in(&home)?;
    Ok(history_status_in(&home))
}

/// Folds what has aged past the cutoff, when a fold is due and every record it needs can be read:
/// every root walked, and every transcript that may hold one read now or from its cached parse.
/// A transcript still being written counts what it holds, since its records old enough to fold
/// were written days ago. Reads only the history file unless a fold is due.
pub(crate) fn fold_history_in(
    home: &Path,
    now_ms: i64,
    checks: &mut FoldChecks,
) -> Result<(), AdapterError> {
    // The history is opened under the lock, so a clear cannot land between this read of it and
    // the fold written over it.
    let lock = lock_usage_files();
    let mut store = HistoryStore::open(history_path_for(home));
    if store.history().is_none() || !fold_due(store.watermark(), now_ms) {
        return Ok(());
    }
    let mut sources = Sources::open(lock, home, || store.watermark());

    let mut saved = Ok(());
    if sources.walked_every_root() {
        let cutoff_ms = fold_cutoff_ms(now_ms);
        let read = sources.read(i64::MIN, store.watermark());
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
    // With the watermark after the fold, so what it folded leaves the scan cache now.
    sources.finish(|| store.watermark());
    saved.map_err(|error| AdapterError::message(format!("Could not save a fold: {error}")))
}

/// How far back the usage history reaches and how much room it takes.
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

/// Forgets every folded row. Usage whose transcript is still on disk is counted from it again and
/// folded again once due; usage only the history held is gone.
pub(crate) fn clear_history_in(home: &Path) -> Result<(), AdapterError> {
    let _lock = lock_usage_files();
    clear_history(&history_path_for(home)).map_err(|error| {
        AdapterError::message(format!("Could not clear the usage history: {error}"))
    })
}

#[cfg(test)]
mod tests;
