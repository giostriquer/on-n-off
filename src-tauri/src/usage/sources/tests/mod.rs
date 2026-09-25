//! The transcript sources through their interface, in a scratch home: what the index answers
//! before any read, and when that answer changes.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::test_support::{
    at, month_start, mtime_ms, record, set_mtime_ms, transcript_path, write_records,
};
use super::*;
use crate::paths::scratch_dir;
use crate::usage::transcripts::USAGE_TRANSCRIPT_PARSER_VERSION;

mod read;
mod watermark;

fn append_record(path: &Path, record: &str) {
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "{record}").unwrap();
}

/// Rewrites `path` with a record of the same length and moves its mtime five seconds on, so only
/// the mtime tells the two versions apart.
fn rewrite_same_size_later(path: &Path, record: &str) {
    let (size, mtime) = (std::fs::metadata(path).unwrap().len(), mtime_ms(path));
    write_records(path, &[record.to_string()]);
    set_mtime_ms(path, mtime + 5_000);
    assert_eq!(std::fs::metadata(path).unwrap().len(), size);
}

/// Sources opened on `home` with `watermark`; the lock is held until they finish.
fn open(home: &Path, watermark: Watermark) -> Sources {
    Sources::open(lock_usage_files(), home, || watermark)
}

fn open_and_finish(home: &Path, watermark: Watermark) -> SeenSources {
    open(home, watermark).finish(|| watermark)
}

/// What `open_and_finish` signs for `[start_ms, end_ms)`.
fn signature(home: &Path, watermark: Watermark, start_ms: i64, end_ms: i64) -> String {
    let sources = open(home, watermark);
    let signature = sources.signature(start_ms, end_ms);
    sources.finish(|| watermark);
    signature
}

fn unchanged(seen: &SeenSources) -> bool {
    seen.unchanged(&lock_usage_files())
}

fn claude_present(seen: &SeenSources) -> bool {
    seen.providers()
        .find(|source| source.provider == UsageProvider::Claude)
        .unwrap()
        .present
}

fn index_file(home: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(source_index_path_for(home)).unwrap()).unwrap()
}

#[test]
fn unchanged_sources_keep_their_index_and_window_signature() {
    let home = scratch_dir("usage-sources-unchanged");
    let path = transcript_path(&home, "august.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    let first = open(&home, Watermark::NONE);
    let first_signature = first.signature(month_start(8), month_start(9));
    let first_seen = first.finish(|| Watermark::NONE);
    assert_eq!(cached_record_count(&home, &path), Some(1));
    reset_transcript_parse_count();

    let second = signature(&home, Watermark::NONE, month_start(8), month_start(9));

    assert_eq!(second, first_signature);
    assert_eq!(transcript_parse_count(), 0);
    assert!(unchanged(&first_seen), "the index was not written again");
    let _ = std::fs::remove_dir_all(home);
}

/// A read made before the transcript was created is stale once the index is saved again, even
/// when the transcripts on disk are back as that read saw them.
#[test]
fn a_created_transcript_changes_the_signature_of_a_window_it_falls_in() {
    let home = scratch_dir("usage-sources-create");
    std::fs::create_dir_all(home.join(".claude").join("projects")).unwrap();
    let before = open(&home, Watermark::NONE);
    let before_signature = before.signature(month_start(8), month_start(9));
    let before_seen = before.finish(|| Watermark::NONE);

    let created_path = transcript_path(&home, "created.jsonl");
    write_records(
        &created_path,
        &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)],
    );
    let created = open(&home, Watermark::NONE);
    let created_signature = created.signature(month_start(8), month_start(9));
    let created_seen = created.finish(|| Watermark::NONE);
    let created_unchanged = unchanged(&created_seen);
    std::fs::remove_file(&created_path).unwrap();

    assert_ne!(created_signature, before_signature);
    assert!(created_unchanged);
    assert!(!unchanged(&before_seen), "the index was saved since");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn an_appended_transcript_changes_the_signature_and_its_cached_parse() {
    let home = scratch_dir("usage-sources-append");
    let path = transcript_path(&home, "append.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    let before = signature(&home, Watermark::NONE, month_start(8), month_start(9));

    append_record(&path, &record("2026-08-08T04:05:13.944Z", "msg-2", 25));
    let appended = signature(&home, Watermark::NONE, month_start(8), month_start(9));

    assert_ne!(appended, before);
    assert_eq!(cached_record_count(&home, &path), Some(2));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_same_size_rewrite_with_a_new_mtime_changes_both_windows_it_moves_between() {
    let home = scratch_dir("usage-sources-rewrite");
    let path = transcript_path(&home, "rewrite.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-a", 20)]);
    let august_before = signature(&home, Watermark::NONE, month_start(8), month_start(9));
    let july_before = signature(&home, Watermark::NONE, month_start(7), month_start(8));

    rewrite_same_size_later(&path, &record("2026-07-07T04:05:13.944Z", "msg-j", 20));

    assert_ne!(
        signature(&home, Watermark::NONE, month_start(8), month_start(9)),
        august_before
    );
    assert_ne!(
        signature(&home, Watermark::NONE, month_start(7), month_start(8)),
        july_before
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_deleted_transcript_changes_the_signature_of_its_previous_window() {
    let home = scratch_dir("usage-sources-delete");
    let path = transcript_path(&home, "delete.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    let before = signature(&home, Watermark::NONE, month_start(8), month_start(9));

    std::fs::remove_file(path).unwrap();

    assert_ne!(
        signature(&home, Watermark::NONE, month_start(8), month_start(9)),
        before
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_window_signature_ignores_a_change_outside_the_window() {
    let home = scratch_dir("usage-sources-disjoint");
    let august_path = transcript_path(&home, "august.jsonl");
    write_records(
        &transcript_path(&home, "july.jsonl"),
        &[record("2026-07-07T04:05:13.944Z", "msg-j", 10)],
    );
    write_records(
        &august_path,
        &[record("2026-08-07T04:05:13.944Z", "msg-a", 20)],
    );
    let july_before = signature(&home, Watermark::NONE, month_start(7), month_start(8));
    let august_before = signature(&home, Watermark::NONE, month_start(8), month_start(9));

    append_record(
        &august_path,
        &record("2026-08-08T04:05:13.944Z", "msg-b", 25),
    );

    assert_eq!(
        signature(&home, Watermark::NONE, month_start(7), month_start(8)),
        july_before
    );
    assert_ne!(
        signature(&home, Watermark::NONE, month_start(8), month_start(9)),
        august_before
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn the_signature_of_no_transcripts_still_depends_on_its_window() {
    let home = scratch_dir("usage-sources-bounds");
    let sources = open(&home, Watermark::NONE);
    let july = sources.signature(month_start(7), month_start(8));
    let august = sources.signature(month_start(8), month_start(9));
    sources.finish(|| Watermark::NONE);
    assert_ne!(july, august);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_root_appearing_or_going_changes_the_signature_and_the_provider_it_shows() {
    let home = scratch_dir("usage-sources-root-presence");
    let missing = open(&home, Watermark::NONE);
    let missing_signature = missing.signature(month_start(8), month_start(9));
    let missing_seen = missing.finish(|| Watermark::NONE);

    std::fs::create_dir_all(home.join(".claude").join("projects")).unwrap();
    let present = open(&home, Watermark::NONE);
    let present_signature = present.signature(month_start(8), month_start(9));
    let present_seen = present.finish(|| Watermark::NONE);

    std::fs::remove_dir_all(home.join(".claude")).unwrap();
    let gone = open(&home, Watermark::NONE);
    let gone_signature = gone.signature(month_start(8), month_start(9));
    let gone_seen = gone.finish(|| Watermark::NONE);

    assert!(!claude_present(&missing_seen));
    assert!(claude_present(&present_seen));
    assert!(!claude_present(&gone_seen));
    assert_ne!(present_signature, missing_signature);
    assert_ne!(gone_signature, present_signature);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn an_incompatible_index_is_rebuilt() {
    let home = scratch_dir("usage-sources-v1-index");
    write_records(
        &transcript_path(&home, "valid.jsonl"),
        &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)],
    );
    let index = source_index_path_for(&home);
    std::fs::create_dir_all(index.parent().unwrap()).unwrap();
    std::fs::write(&index, r#"{"version":1,"generation":99,"entries":{}}"#).unwrap();

    let sources = open(&home, Watermark::NONE);
    let complete = sources.is_complete();
    sources.finish(|| Watermark::NONE);

    assert!(complete);
    let rebuilt = index_file(&home);
    assert_eq!(rebuilt["version"], json!(USAGE_SOURCE_INDEX_VERSION));
    assert_eq!(
        rebuilt["parserVersion"],
        json!(USAGE_TRANSCRIPT_PARSER_VERSION)
    );
    assert_eq!(rebuilt["scanCacheVersion"], json!(USAGE_SCAN_CACHE_VERSION));
    assert_eq!(rebuilt["generation"], json!(1));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_corrupt_index_is_rebuilt_from_the_transcripts() {
    let home = scratch_dir("usage-sources-corrupt-index");
    let path = transcript_path(&home, "valid.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    let index = source_index_path_for(&home);
    std::fs::create_dir_all(index.parent().unwrap()).unwrap();
    std::fs::write(&index, "{partial").unwrap();

    open_and_finish(&home, Watermark::NONE);

    let rebuilt = index_file(&home);
    assert_eq!(rebuilt["generation"], json!(1));
    assert_eq!(rebuilt["entries"].as_object().unwrap().len(), 1);
    assert_eq!(cached_record_count(&home, &path), Some(1));
    let _ = std::fs::remove_dir_all(home);
}
