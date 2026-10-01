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

pub const FOLD_AFTER_DAYS: i64 = 7;

const SLOT_MS: i64 = 15 * 60 * 1000;
pub const DAY_MS: i64 = 24 * 60 * 60 * 1000;

const LONG_CONTEXT_INPUT_TOKENS: u64 = 200_000;

const REPORTED_FLAG: u8 = 1;
const LONG_CONTEXT_FLAG: u8 = 2;

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

    pub fn is_folded(self, at_ms: i64) -> bool {
        at_ms < self.0
    }

    pub fn holds_only_folded(self, mtime_ms: i64, newest_record_ms: Option<i64>) -> bool {
        newest_record_ms.is_some_and(|newest| self.is_folded(newest))
            || mtime_ms < self.0.saturating_sub(MTIME_SLACK_MS)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FoldedRow {
    pub slot_start_ms: i64,
    pub provider: UsageProvider,
    pub model: String,
    pub reported: bool,
    pub long_context: bool,
    pub totals: TokenTotals,
    pub records: u64,
    pub reported_cost_usd: f64,
    pub sessions: Vec<String>,
}

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

impl UsageHistory {
    pub fn watermark(&self) -> Watermark {
        self.folded_through_ms.map_or(Watermark::NONE, Watermark)
    }

    #[cfg(test)]
    pub fn rows(&self) -> &[FoldedRow] {
        &self.rows
    }

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

#[derive(Debug)]
pub struct HistoryStore {
    path: PathBuf,
    state: StoreState,
}

#[derive(Debug)]
enum StoreState {
    Readable {
        history: UsageHistory,
        recovered: bool,
    },
    Unreadable,
}

impl HistoryStore {
    pub fn open(path: PathBuf) -> Self {
        let state = load(&path);
        Self { path, state }
    }

    pub fn history(&self) -> Option<&UsageHistory> {
        match &self.state {
            StoreState::Readable { history, .. } => Some(history),
            StoreState::Unreadable => None,
        }
    }

    pub fn watermark(&self) -> Watermark {
        self.history()
            .map_or(Watermark::NONE, UsageHistory::watermark)
    }

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
    match std::fs::copy(path, target) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

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

pub fn clear_history(path: &Path) -> io::Result<()> {
    let copies = copies_of(path)?;
    for target in copies.iter().map(PathBuf::as_path).chain([path]) {
        match std::fs::remove_file(target) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

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
