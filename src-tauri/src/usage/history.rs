//! Usage kept after the agents delete their transcripts.
//!
//! Claude Code deletes a transcript once it is older than `cleanupPeriodDays` (30 by default), so
//! a read that only ever parses transcripts loses that usage for good. Records older than a
//! cutoff are folded here instead: summed into rows keyed by a 15-minute UTC slot, provider,
//! model and how the record is priced, holding token totals, record count, provider-reported
//! cost and the session ids seen in the slot. Every UTC offset in use is a multiple of 15
//! minutes, so a slot never straddles a local midnight or hour, and day and hour buckets in any
//! time zone come out as they would from the records themselves. Tokens are kept rather than
//! dollars, so a new price table still prices history.
//!
//! The file also holds the watermark: rows cover every record before it, and a read counts a
//! transcript's records only from it onward. Nothing below the watermark is ever counted twice,
//! and a transcript entirely below it is never parsed again. The watermark only moves forward,
//! to the UTC midnight [`FOLD_AFTER_DAYS`] before now, well inside every provider's retention.
//!
//! This is user data, not a cache: it cannot be rebuilt once the transcripts are gone. A file
//! that does not read is never written over: the previous good file is kept as a backup and read
//! in its place, and the unreadable one is set aside, never deleted. A file a newer on-n-off
//! wrote is left alone entirely.

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::cache_io::atomic_write;
use super::transcripts::{
    TokenTotals, UsageProvider, UsageRecord, USAGE_TRANSCRIPT_PARSER_VERSION,
};

pub const USAGE_HISTORY_VERSION: u32 = 1;

/// How old a record must be before it is folded. Records newer than this stay in the transcripts,
/// where a parser fix still reaches them.
pub const FOLD_AFTER_DAYS: i64 = 7;

const SLOT_MS: i64 = 15 * 60 * 1000;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// A request whose input (fresh, cached and cache writes) exceeds this is flagged in its row.
/// LiteLLM lists a higher `*_above_200k_tokens` rate for some models; on-n-off does not apply it
/// yet, and a slot's sum could not say which requests crossed the line once folded.
const LONG_CONTEXT_INPUT_TOKENS: u64 = 200_000;

const REPORTED_FLAG: u8 = 1;
const LONG_CONTEXT_FLAG: u8 = 2;

/// The records of one slot that share a provider, a model and a way of being priced.
#[derive(Debug, Clone, PartialEq)]
pub struct FoldedRow {
    pub slot_start_ms: i64,
    pub provider: UsageProvider,
    pub model: String,
    /// Every record carried a provider-reported cost, summed in `reported_cost_usd`; the others
    /// are priced from their tokens at read time.
    pub reported: bool,
    pub long_context: bool,
    pub totals: TokenTotals,
    pub records: u64,
    pub reported_cost_usd: f64,
    /// Sorted, each once, never empty strings.
    pub sessions: Vec<String>,
}

/// One fold: which records it took and which parser read them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoldSegment {
    pub from_ms: Option<i64>,
    pub to_ms: i64,
    pub parser_version: u32,
    pub folded_at_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageHistory {
    folded_through_ms: Option<i64>,
    segments: Vec<FoldSegment>,
    rows: Vec<FoldedRow>,
}

#[derive(Debug)]
pub enum LoadedHistory {
    /// Nothing folded yet.
    Missing,
    Ready {
        history: UsageHistory,
        /// Read from the backup because the file itself did not read; the next write sets the
        /// file aside first.
        recovered: bool,
    },
    /// Neither the file nor its backup reads, or a newer on-n-off wrote it: never fold over it
    /// or write it.
    Unreadable,
}

#[derive(Debug)]
pub enum DecodeError {
    Newer,
    Damaged,
}

impl UsageHistory {
    /// Rows cover every record before this instant; `None` before the first fold.
    pub fn folded_through_ms(&self) -> Option<i64> {
        self.folded_through_ms
    }

    pub fn rows(&self) -> &[FoldedRow] {
        &self.rows
    }

    pub fn kept_since_ms(&self) -> Option<i64> {
        self.rows.iter().map(|row| row.slot_start_ms).min()
    }

    /// Folds the records from the watermark up to `cutoff_ms` and moves the watermark there.
    /// `records` must already be one per copy group (`transcripts::richest_copies`); records
    /// before the watermark are already folded and are skipped. A cutoff at or before the
    /// watermark changes nothing.
    pub fn fold<'a>(
        &mut self,
        records: impl IntoIterator<Item = &'a UsageRecord>,
        cutoff_ms: i64,
        folded_at_ms: i64,
    ) {
        let from_ms = self.folded_through_ms;
        if from_ms.is_some_and(|through| cutoff_ms <= through) {
            return;
        }

        let mut groups: HashMap<RowKey<'a>, RowSums<'a>> = HashMap::new();
        for record in records {
            let at = record.timestamp_ms;
            if from_ms.is_some_and(|through| at < through) || at >= cutoff_ms {
                continue;
            }
            let reported_cost = record.reported_cost_usd.filter(|cost| cost.is_finite());
            let key = RowKey {
                slot_start_ms: at.div_euclid(SLOT_MS) * SLOT_MS,
                provider: record.provider,
                model: &record.model,
                reported: reported_cost.is_some(),
                long_context: input_tokens(&record.totals) > LONG_CONTEXT_INPUT_TOKENS,
            };
            let sums = groups.entry(key).or_default();
            sums.totals = sums.totals.add(&record.totals);
            sums.records += 1;
            sums.reported_cost_usd += reported_cost.unwrap_or(0.0);
            if !record.session_id.is_empty() {
                sums.sessions.insert(&record.session_id);
            }
        }

        let mut rows: Vec<FoldedRow> = groups
            .into_iter()
            .map(|(key, sums)| FoldedRow {
                slot_start_ms: key.slot_start_ms,
                provider: key.provider,
                model: key.model.to_string(),
                reported: key.reported,
                long_context: key.long_context,
                totals: sums.totals,
                records: sums.records,
                reported_cost_usd: sums.reported_cost_usd,
                sessions: sums.sessions.into_iter().map(str::to_string).collect(),
            })
            .collect();
        rows.sort_by(|a, b| row_order(a).cmp(&row_order(b)));
        self.rows.extend(rows);
        self.record_segment(from_ms, cutoff_ms, folded_at_ms);
        self.folded_through_ms = Some(cutoff_ms);
    }

    fn record_segment(&mut self, from_ms: Option<i64>, to_ms: i64, folded_at_ms: i64) {
        if let Some(last) = self.segments.last_mut() {
            if last.parser_version == USAGE_TRANSCRIPT_PARSER_VERSION && Some(last.to_ms) == from_ms
            {
                last.to_ms = to_ms;
                last.folded_at_ms = folded_at_ms;
                return;
            }
        }
        self.segments.push(FoldSegment {
            from_ms,
            to_ms,
            parser_version: USAGE_TRANSCRIPT_PARSER_VERSION,
            folded_at_ms,
        });
    }
}

#[derive(PartialEq, Eq, Hash)]
struct RowKey<'a> {
    slot_start_ms: i64,
    provider: UsageProvider,
    model: &'a str,
    reported: bool,
    long_context: bool,
}

#[derive(Default)]
struct RowSums<'a> {
    totals: TokenTotals,
    records: u64,
    reported_cost_usd: f64,
    sessions: BTreeSet<&'a str>,
}

fn row_order(row: &FoldedRow) -> (i64, &str, &str, bool, bool) {
    (
        row.slot_start_ms,
        row.provider.as_str(),
        &row.model,
        row.reported,
        row.long_context,
    )
}

fn input_tokens(totals: &TokenTotals) -> u64 {
    totals.uncached_input_tokens + totals.cached_input_tokens + totals.cache_creation_tokens
}

/// The UTC midnight [`FOLD_AFTER_DAYS`] before `now_ms`. Moving once a day keeps folds, and the
/// transcript reads they need, to one a day.
pub fn fold_cutoff_ms(now_ms: i64) -> i64 {
    (now_ms - FOLD_AFTER_DAYS * DAY_MS).div_euclid(DAY_MS) * DAY_MS
}

/// A transcript last written this long before the watermark holds no record after it, even with
/// its records stamped by a clock that disagrees with the filesystem's.
const FOLDED_MTIME_SLACK_MS: i64 = 36 * 60 * 60 * 1000;

/// Whether every record a transcript holds is below the watermark, so the history already has
/// them and the transcript need not be read: its newest record is, or it was last written well
/// before.
pub fn holds_only_folded(
    mtime_ms: i64,
    newest_record_ms: Option<i64>,
    folded_through_ms: Option<i64>,
) -> bool {
    folded_through_ms.is_some_and(|through| {
        newest_record_ms.is_some_and(|newest| newest < through)
            || mtime_ms < through - FOLDED_MTIME_SLACK_MS
    })
}

pub fn fold_due(folded_through_ms: Option<i64>, now_ms: i64) -> bool {
    folded_through_ms.is_none_or(|through| fold_cutoff_ms(now_ms) > through)
}

pub fn history_path_for(home: &Path) -> PathBuf {
    home.join(".on-n-off").join("usage-history.json")
}

fn backup_path(path: &Path) -> PathBuf {
    sibling(path, "bak")
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}

pub fn load_history(path: &Path) -> LoadedHistory {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return LoadedHistory::Missing,
        Err(_) => return LoadedHistory::Unreadable,
    };
    match decode(&raw) {
        Ok(history) => LoadedHistory::Ready {
            history,
            recovered: false,
        },
        Err(DecodeError::Newer) => LoadedHistory::Unreadable,
        Err(DecodeError::Damaged) => std::fs::read_to_string(backup_path(path))
            .ok()
            .and_then(|raw| decode(&raw).ok())
            .map_or(LoadedHistory::Unreadable, |history| LoadedHistory::Ready {
                history,
                recovered: true,
            }),
    }
}

/// Writes `history`, keeping the file it replaces as the backup. A `recovered` history came from
/// the backup because the file did not read: that file is copied aside under a new name first,
/// and the backup is not replaced with it.
pub fn persist_history(path: &Path, history: &UsageHistory, recovered: bool) -> io::Result<()> {
    persist_history_with(path, history, recovered, atomic_write)
}

/// [`persist_history`] with the write of the file itself supplied, so a test can fail it.
fn persist_history_with(
    path: &Path,
    history: &UsageHistory,
    recovered: bool,
    write_file: impl FnOnce(&Path, &str) -> io::Result<()>,
) -> io::Result<()> {
    if recovered {
        set_aside(path)?;
    } else {
        match std::fs::read_to_string(path) {
            Ok(previous) => atomic_write(&backup_path(path), &previous)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    write_file(path, &encode(history))
}

fn set_aside(path: &Path) -> io::Result<()> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let mut attempt = 0u32;
    let target = loop {
        let candidate = sibling(path, &format!("unreadable-{stamp}-{attempt}"));
        if !candidate.exists() {
            break candidate;
        }
        attempt += 1;
    };
    // Copied, not moved: the file stays where it is until the new one replaces it, so a write
    // that fails leaves it to fall back to the backup again.
    match std::fs::copy(path, target) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Forgets every folded row: the file, its backup, and any copy set aside.
pub fn clear_history(path: &Path) -> io::Result<()> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(());
    };
    let prefix = format!("{}.", name.to_string_lossy());
    let copies = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|candidate| {
                candidate
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
            })
            .collect(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    // The file goes last, so a clear cut short leaves the history readable rather than half gone.
    for target in copies.iter().map(PathBuf::as_path).chain([path]) {
        match std::fs::remove_file(target) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryFile {
    version: u32,
    folded_through_ms: Option<i64>,
    segments: Vec<FoldSegment>,
    models: Vec<String>,
    sessions: Vec<String>,
    rows: Vec<WireRow>,
}

/// `[slot start, provider, model index, flags, uncached input, cached input, cache writes,
/// one-hour cache writes, output, reasoning, records, reported cost, session indexes]`
#[derive(Serialize, Deserialize)]
struct WireRow(
    i64,
    UsageProvider,
    usize,
    u8,
    u64,
    u64,
    u64,
    u64,
    u64,
    u64,
    u64,
    f64,
    Vec<usize>,
);

fn encode(history: &UsageHistory) -> String {
    let mut models = Interner::default();
    let mut sessions = Interner::default();
    let rows = history
        .rows
        .iter()
        .map(|row| {
            let flags = if row.reported { REPORTED_FLAG } else { 0 }
                | if row.long_context {
                    LONG_CONTEXT_FLAG
                } else {
                    0
                };
            let t = &row.totals;
            WireRow(
                row.slot_start_ms,
                row.provider,
                models.index(&row.model),
                flags,
                t.uncached_input_tokens,
                t.cached_input_tokens,
                t.cache_creation_tokens,
                t.cache_creation_1h_tokens,
                t.output_tokens,
                t.reasoning_tokens,
                row.records,
                row.reported_cost_usd,
                row.sessions
                    .iter()
                    .map(|session| sessions.index(session))
                    .collect(),
            )
        })
        .collect();
    let file = HistoryFile {
        version: USAGE_HISTORY_VERSION,
        folded_through_ms: history.folded_through_ms,
        segments: history.segments.clone(),
        models: models.values,
        sessions: sessions.values,
        rows,
    };
    serde_json::to_string(&file).expect("usage history always serializes")
}

#[derive(Default)]
struct Interner {
    values: Vec<String>,
    index: HashMap<String, usize>,
}

impl Interner {
    fn index(&mut self, value: &str) -> usize {
        if let Some(&index) = self.index.get(value) {
            return index;
        }
        let next = self.values.len();
        self.values.push(value.to_string());
        self.index.insert(value.to_string(), next);
        next
    }
}

/// Every row or none: a row that does not read makes the whole file unreadable, because writing
/// the rest back would lose that row's usage for good.
fn decode(raw: &str) -> Result<UsageHistory, DecodeError> {
    let document: Value = serde_json::from_str(raw).map_err(|_| DecodeError::Damaged)?;
    let version = document
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(DecodeError::Damaged)?;
    if version > u64::from(USAGE_HISTORY_VERSION) {
        return Err(DecodeError::Newer);
    }
    if version != u64::from(USAGE_HISTORY_VERSION) {
        return Err(DecodeError::Damaged);
    }
    let file: HistoryFile = serde_json::from_value(document).map_err(|_| DecodeError::Damaged)?;
    let rows = file
        .rows
        .into_iter()
        .map(|row| decode_row(row, &file.models, &file.sessions))
        .collect::<Option<Vec<_>>>()
        .ok_or(DecodeError::Damaged)?;
    Ok(UsageHistory {
        folded_through_ms: file.folded_through_ms,
        segments: file.segments,
        rows,
    })
}

fn decode_row(row: WireRow, models: &[String], sessions: &[String]) -> Option<FoldedRow> {
    let WireRow(
        slot_start_ms,
        provider,
        model,
        flags,
        uncached_input_tokens,
        cached_input_tokens,
        cache_creation_tokens,
        cache_creation_1h_tokens,
        output_tokens,
        reasoning_tokens,
        records,
        reported_cost_usd,
        session_indexes,
    ) = row;
    if flags & !(REPORTED_FLAG | LONG_CONTEXT_FLAG) != 0
        || cache_creation_1h_tokens > cache_creation_tokens
        || !reported_cost_usd.is_finite()
    {
        return None;
    }
    Some(FoldedRow {
        slot_start_ms,
        provider,
        model: models.get(model)?.clone(),
        reported: flags & REPORTED_FLAG != 0,
        long_context: flags & LONG_CONTEXT_FLAG != 0,
        totals: TokenTotals {
            uncached_input_tokens,
            cached_input_tokens,
            cache_creation_tokens,
            cache_creation_1h_tokens,
            output_tokens,
            reasoning_tokens,
        },
        records,
        reported_cost_usd,
        sessions: session_indexes
            .into_iter()
            .map(|index| sessions.get(index).cloned())
            .collect::<Option<_>>()?,
    })
}

#[cfg(test)]
mod tests;
