mod history;
mod sources;
mod summary_cache;

use std::cell::OnceCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{SecondsFormat, TimeZone, Utc};

use crate::dto::{AdapterError, UsageHistoryState, UsageHistoryStatusDto, UsageSummaryDto};
use crate::paths::user_home;
use crate::provider_catalogue::UsageProvider;

use super::transcripts::UsageRecord;
use super::window::{Span, UsageWindow};
use history::{
    clear_history, fold_cutoff_ms, fold_due, history_bytes, history_fingerprint, history_path_for,
    HistoryStore, DAY_MS, FOLD_AFTER_DAYS,
};
use sources::{SeenSources, SourceRead, Sources};
use summary_cache::{load_summary_hit, store_summary, summary_cache_path_for, summary_key};

pub use history::FoldedRow;
pub use sources::FilesRead;

#[cfg(test)]
pub(crate) mod testing {
    pub(crate) use super::history::{
        history_open_count, history_path_for, reset_history_open_count, HistoryStore, UsageHistory,
        Watermark,
    };
    #[cfg(unix)]
    pub(crate) use super::sources::scan_cache_file;
    pub(crate) use super::sources::{
        cached_record_count, reset_scan_cache_decode_count, reset_transcript_parse_count,
        scan_cache_decode_count, source_index_file, transcript_parse_count,
        USAGE_SCAN_CACHE_VERSION, USAGE_SOURCE_INDEX_VERSION,
    };
    pub(crate) use super::summary_cache::{summary_cache_path_for, USAGE_SUMMARY_CACHE_VERSION};
}

const UNREADABLE_GRACE_MS: i64 = FOLD_AFTER_DAYS * DAY_MS;

pub(crate) struct Locked {
    _guard: MutexGuard<'static, ()>,
}

fn lock() -> Locked {
    static LOCK: Mutex<()> = Mutex::new(());
    Locked {
        _guard: LOCK.lock().unwrap_or_else(PoisonError::into_inner),
    }
}

pub struct UsageFiles {
    home: PathBuf,
    history_path: PathBuf,
    summary_cache: PathBuf,
}

pub struct WindowCount {
    read: SourceRead,
    history: HistoryStore,
    counted_fingerprint: String,
    span: Span,
    seen: SeenSources,
}

pub struct ProviderRead<'a> {
    pub provider: UsageProvider,
    pub dir: &'a Path,
    pub files: Option<FilesRead>,
}

#[derive(Debug, Default)]
pub struct FoldChecks {
    unread_last_time: HashSet<String>,
}

impl UsageFiles {
    pub fn of(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            history_path: history_path_for(home),
            summary_cache: summary_cache_path_for(home),
        }
    }

    pub fn summary(
        &self,
        window: &UsageWindow,
        rates_fetched_at_ms: Option<i64>,
        force: bool,
        count: impl FnOnce(&WindowCount) -> UsageSummaryDto,
    ) -> UsageSummaryDto {
        let span = window.span();
        pause_before(Step::Count);
        let locked = lock();
        pause_before(Step::Read);
        let counted_fingerprint = history_fingerprint(&locked, &self.history_path);
        let key = summary_key(
            &window.cache_identity(),
            rates_fetched_at_ms,
            &counted_fingerprint,
        );
        let open_history = || HistoryStore::open(self.history_path.clone());
        let opened = OnceCell::new();
        let mut sources = Sources::open(&locked, &self.home, || {
            opened.get_or_init(open_history).watermark()
        });
        let signature = sources.signature(span);
        if sources.is_complete() && !force {
            if let Some(hit) = load_summary_hit(&self.summary_cache, &key, &signature) {
                sources.finish(|| opened.get_or_init(open_history).watermark());
                return hit;
            }
        }
        let history = opened.into_inner().unwrap_or_else(open_history);
        let read = sources.read(window.earliest_mtime_ms(), || history.watermark());
        let seen = sources.finish(|| history.watermark());
        drop(locked);

        let counted = WindowCount {
            read,
            history,
            counted_fingerprint,
            span,
            seen,
        };
        let summary = count(&counted);

        pause_before(Step::Store);
        let locked = lock();
        if counted.is_current(&locked, &self.history_path) {
            store_summary(&locked, &self.summary_cache, &key, &signature, &summary);
        }
        summary
    }

    pub fn fold(&self, now_ms: i64, checks: &mut FoldChecks) -> Result<(), AdapterError> {
        let locked = lock();
        let mut history = HistoryStore::open(self.history_path.clone());
        if history.history().is_none() || !fold_due(history.watermark(), now_ms) {
            return Ok(());
        }
        let mut sources = Sources::open(&locked, &self.home, || history.watermark());

        let mut saved = Ok(());
        if sources.walked_every_root() {
            let cutoff_ms = fold_cutoff_ms(now_ms);
            let read = sources.read(i64::MIN, || history.watermark());
            if !checks.wait_for(&read.unread_files, cutoff_ms) {
                saved = history.fold(&locked, read.records(), cutoff_ms, now_ms);
            }
        }
        sources.finish(|| history.watermark());
        saved.map_err(|error| AdapterError::message(format!("Could not save a fold: {error}")))
    }

    pub fn clear(&self) -> Result<(), AdapterError> {
        clear_history(&lock(), &self.history_path).map_err(|error| {
            AdapterError::message(format!("Could not clear the usage history: {error}"))
        })
    }

    pub fn status(&self) -> UsageHistoryStatusDto {
        let store = HistoryStore::open(self.history_path.clone());
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
            bytes: history_bytes(&self.history_path),
        }
    }
}

impl WindowCount {
    pub fn records(&self) -> Vec<&UsageRecord> {
        self.read.records()
    }

    pub fn folded_rows(&self) -> &[FoldedRow] {
        self.history
            .history()
            .map_or(&[][..], |history| history.rows_in(self.span))
    }

    pub fn providers(&self) -> impl Iterator<Item = ProviderRead<'_>> {
        self.seen.providers(&self.read)
    }

    fn is_current(&self, locked: &Locked, history_path: &Path) -> bool {
        self.read.complete
            && self.history.history().is_some()
            && self.seen.unchanged(locked)
            && history_fingerprint(locked, history_path) == self.counted_fingerprint
    }
}

impl FoldChecks {
    fn wait_for(&mut self, unread_files: &[(String, i64)], cutoff_ms: i64) -> bool {
        let waiting = unread_files.iter().any(|(path, mtime_ms)| {
            *mtime_ms >= cutoff_ms - UNREADABLE_GRACE_MS || !self.unread_last_time.contains(path)
        });
        self.unread_last_time = unread_files.iter().map(|(path, _)| path.clone()).collect();
        waiting
    }
}

pub fn usage_history_status() -> Result<UsageHistoryStatusDto, AdapterError> {
    Ok(UsageFiles::of(&user_home()?).status())
}

pub fn clear_usage_history() -> Result<UsageHistoryStatusDto, AdapterError> {
    let files = UsageFiles::of(&user_home()?);
    files.clear()?;
    Ok(files.status())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Count,
    Read,
    Store,
}

#[cfg(test)]
type Pause = (
    Step,
    std::sync::mpsc::Sender<()>,
    std::sync::mpsc::Receiver<()>,
);

#[cfg(test)]
thread_local! {
    static PAUSE: std::cell::RefCell<Option<Pause>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn with_pause_before<R>(
    step: Step,
    reached: std::sync::mpsc::Sender<()>,
    resume: std::sync::mpsc::Receiver<()>,
    action: impl FnOnce() -> R,
) -> R {
    PAUSE.with(|pause| *pause.borrow_mut() = Some((step, reached, resume)));
    let result = action();
    PAUSE.with(|pause| *pause.borrow_mut() = None);
    result
}

#[cfg(test)]
fn pause_before(step: Step) {
    PAUSE.with(|pause| {
        if let Some((at, reached, resume)) = pause.borrow().as_ref() {
            if *at == step {
                let _ = reached.send(());
                let _ = resume.recv();
            }
        }
    });
}

#[cfg(not(test))]
fn pause_before(_step: Step) {}

#[cfg(test)]
mod tests;
