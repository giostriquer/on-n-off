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
//! The file also holds the [`Watermark`]: rows cover every record before it, and a read counts a
//! transcript's records only from it onward. Nothing below the watermark is ever counted twice,
//! and a transcript entirely below it is never parsed again. The watermark only moves forward,
//! to the UTC midnight [`FOLD_AFTER_DAYS`] before now, well inside every provider's retention.
//!
//! This is user data, not a cache: it cannot be rebuilt once the transcripts are gone. A file
//! that does not read is never written over: the previous good file is kept as a backup and read
//! in its place, and the unreadable one is copied aside, never deleted. A file a newer on-n-off
//! wrote is left alone entirely, and a history is written only once it is known to read back.

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::cache_io::atomic_write;
use super::reader::MTIME_SLACK_MS;
use super::transcripts::{
    TokenTotals, UsageProvider, UsageRecord, USAGE_TRANSCRIPT_PARSER_VERSION,
};

pub const USAGE_HISTORY_VERSION: u32 = 1;

/// How old a record must be before it is folded. Records newer than this stay in the transcripts,
/// where a parser fix still reaches them.
pub const FOLD_AFTER_DAYS: i64 = 7;

const SLOT_MS: i64 = 15 * 60 * 1000;
pub const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// A request whose input (fresh, cached and cache writes) exceeds this is flagged in its row.
/// LiteLLM lists a higher `*_above_200k_tokens` rate for some models; on-n-off does not apply it
/// yet, and a slot's sum could not say which requests crossed the line once folded.
const LONG_CONTEXT_INPUT_TOKENS: u64 = 200_000;

const REPORTED_FLAG: u8 = 1;
const LONG_CONTEXT_FLAG: u8 = 2;

/// The instant before which the history holds every record and the transcripts count for
/// nothing. [`Watermark::NONE`] until the first fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Watermark(i64);

impl Watermark {
    pub const NONE: Self = Self(i64::MIN);

    #[cfg(test)]
    pub fn at(ms: i64) -> Self {
        Self(ms)
    }

    pub fn ms(self) -> Option<i64> {
        (self != Self::NONE).then_some(self.0)
    }

    /// A record from `at_ms` is in the history, not counted from its transcript.
    pub fn is_folded(self, at_ms: i64) -> bool {
        at_ms < self.0
    }

    /// Every record a transcript holds is in the history, so the transcript need not be read:
    /// its newest record is, or it was last written well before the watermark.
    pub fn holds_only_folded(self, mtime_ms: i64, newest_record_ms: Option<i64>) -> bool {
        newest_record_ms.is_some_and(|newest| self.is_folded(newest))
            || mtime_ms < self.0.saturating_sub(MTIME_SLACK_MS)
    }
}

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

/// Rows sorted by slot, all before the watermark.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageHistory {
    folded_through_ms: Option<i64>,
    segments: Vec<FoldSegment>,
    rows: Vec<FoldedRow>,
}

impl UsageHistory {
    pub fn watermark(&self) -> Watermark {
        self.folded_through_ms.map_or(Watermark::NONE, Watermark)
    }

    #[cfg(test)]
    pub fn rows(&self) -> &[FoldedRow] {
        &self.rows
    }

    /// The rows whose slot starts in `[start_ms, end_ms)`.
    pub fn rows_between(&self, start_ms: i64, end_ms: i64) -> &[FoldedRow] {
        let first = self
            .rows
            .partition_point(|row| row.slot_start_ms < start_ms);
        let end = self.rows.partition_point(|row| row.slot_start_ms < end_ms);
        &self.rows[first..end.max(first)]
    }

    pub fn kept_since_ms(&self) -> Option<i64> {
        self.rows.first().map(|row| row.slot_start_ms)
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
        let from = self.watermark();
        if from.ms().is_some_and(|through| cutoff_ms <= through) {
            return;
        }

        let mut groups: HashMap<RowKey<'a>, RowSums<'a>> = HashMap::new();
        for record in records {
            let at = record.timestamp_ms;
            if from.is_folded(at) || at >= cutoff_ms {
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
        // Every new slot starts at or after the old watermark, past every row already held.
        self.rows.extend(rows);
        self.record_segment(from.ms(), cutoff_ms, folded_at_ms);
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

pub fn fold_due(watermark: Watermark, now_ms: i64) -> bool {
    watermark
        .ms()
        .is_none_or(|through| fold_cutoff_ms(now_ms) > through)
}

pub fn history_path_for(home: &Path) -> PathBuf {
    home.join(".on-n-off").join("usage-history.json")
}

/// The history file and what it holds. A missing file is an empty history; one that does not
/// read, with no readable backup, is held apart and never written.
#[derive(Debug)]
pub struct HistoryStore {
    path: PathBuf,
    state: StoreState,
}

#[derive(Debug)]
enum StoreState {
    Readable {
        history: UsageHistory,
        /// Read from the backup because the file itself did not read; the next save copies
        /// that file aside first.
        recovered: bool,
    },
    Unreadable,
}

impl HistoryStore {
    pub fn open(path: PathBuf) -> Self {
        let state = load(&path);
        Self { path, state }
    }

    /// `None` when the file does not read.
    pub fn history(&self) -> Option<&UsageHistory> {
        match &self.state {
            StoreState::Readable { history, .. } => Some(history),
            StoreState::Unreadable => None,
        }
    }

    /// [`Watermark::NONE`] when the file does not read: every record counts from its transcript.
    pub fn watermark(&self) -> Watermark {
        self.history()
            .map_or(Watermark::NONE, UsageHistory::watermark)
    }

    /// Folds into a copy and saves it; the store holds the fold only once the file has it, so a
    /// save that fails changes nothing. A file that does not read is never folded over.
    pub fn fold<'a>(
        &mut self,
        records: impl IntoIterator<Item = &'a UsageRecord>,
        cutoff_ms: i64,
        folded_at_ms: i64,
    ) -> io::Result<()> {
        let StoreState::Readable { history, recovered } = &mut self.state else {
            return Err(io::Error::other("the usage history does not read"));
        };
        let mut folded = history.clone();
        folded.fold(records, cutoff_ms, folded_at_ms);
        save(&self.path, &folded, *recovered)?;
        *history = folded;
        *recovered = false;
        Ok(())
    }
}

fn load(path: &Path) -> StoreState {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return StoreState::Readable {
                history: UsageHistory::default(),
                recovered: false,
            }
        }
        Err(_) => return StoreState::Unreadable,
    };
    match decode(&raw) {
        Ok(history) => StoreState::Readable {
            history,
            recovered: false,
        },
        Err(DecodeError::Newer) => StoreState::Unreadable,
        Err(DecodeError::Damaged) => std::fs::read_to_string(backup_path(path))
            .ok()
            .and_then(|raw| decode(&raw).ok())
            .map_or(StoreState::Unreadable, |history| StoreState::Readable {
                history,
                recovered: true,
            }),
    }
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

fn save(path: &Path, history: &UsageHistory, recovered: bool) -> io::Result<()> {
    save_with(path, history, recovered, atomic_write)
}

/// Writes `history`, keeping the file it replaces as the backup. A `recovered` history came from
/// the backup because the file did not read: that file is copied aside under a new name first,
/// and the backup is not replaced with it. Nothing is touched unless the encoded history reads
/// back as itself. `write_file` writes the file itself, so a test can fail it.
fn save_with(
    path: &Path,
    history: &UsageHistory,
    recovered: bool,
    write_file: impl FnOnce(&Path, &str) -> io::Result<()>,
) -> io::Result<()> {
    let encoded = encode(history);
    if decode(&encoded).ok().as_ref() != Some(history) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the usage history would not read back",
        ));
    }
    if recovered {
        set_aside(path)?;
    } else {
        match std::fs::read_to_string(path) {
            Ok(previous) => atomic_write(&backup_path(path), &previous)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    write_file(path, &encoded)
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

/// The history file's backup, the copies set aside and any temporary left by an interrupted
/// write: every sibling named after it.
fn copies_of(path: &Path) -> io::Result<Vec<PathBuf>> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(Vec::new());
    };
    let prefix = format!("{}.", name.to_string_lossy());
    match std::fs::read_dir(dir) {
        Ok(entries) => Ok(entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|candidate| {
                candidate
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
            })
            .collect()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

/// The room the history takes on disk: the file and every copy of it.
pub fn history_bytes(path: &Path) -> u64 {
    copies_of(path)
        .unwrap_or_default()
        .iter()
        .map(PathBuf::as_path)
        .chain([path])
        .filter_map(|file| std::fs::metadata(file).ok())
        .map(|metadata| metadata.len())
        .sum()
}

/// Forgets every folded row: the file, its backup, and any copy set aside.
pub fn clear_history(path: &Path) -> io::Result<()> {
    let copies = copies_of(path)?;
    // The file goes last, so a clear cut short leaves the history readable rather than half gone.
    for target in copies.iter().map(PathBuf::as_path).chain([path]) {
        match std::fs::remove_file(target) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

/// Changes whenever the history file is written or removed, so a stored summary counted with
/// one history is never served with another.
pub fn history_fingerprint(path: &Path) -> String {
    std::fs::metadata(path).map_or_else(
        |_| "none".to_string(),
        |metadata| {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |elapsed| elapsed.as_nanos());
            format!("{}:{modified}", metadata.len())
        },
    )
}

#[derive(Debug)]
enum DecodeError {
    Newer,
    Damaged,
}

#[derive(Deserialize)]
struct VersionOnly {
    version: u64,
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
/// the rest back would lose that row's usage for good. So do rows out of slot order, or at or
/// past the watermark, which a read would count a second time from the transcripts.
fn decode(raw: &str) -> Result<UsageHistory, DecodeError> {
    let version = serde_json::from_str::<VersionOnly>(raw)
        .map_err(|_| DecodeError::Damaged)?
        .version;
    if version > u64::from(USAGE_HISTORY_VERSION) {
        return Err(DecodeError::Newer);
    }
    if version != u64::from(USAGE_HISTORY_VERSION) {
        return Err(DecodeError::Damaged);
    }
    let file: HistoryFile = serde_json::from_str(raw).map_err(|_| DecodeError::Damaged)?;
    let rows = file
        .rows
        .into_iter()
        .map(|row| decode_row(row, &file.models, &file.sessions))
        .collect::<Option<Vec<_>>>()
        .ok_or(DecodeError::Damaged)?;
    let in_slot_order = rows
        .windows(2)
        .all(|pair| pair[0].slot_start_ms <= pair[1].slot_start_ms);
    let below_watermark = rows.last().is_none_or(|last| {
        file.folded_through_ms
            .is_some_and(|through| last.slot_start_ms < through)
    });
    if !in_slot_order || !below_watermark {
        return Err(DecodeError::Damaged);
    }
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
