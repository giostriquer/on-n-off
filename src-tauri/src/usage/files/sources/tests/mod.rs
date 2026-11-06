use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::test_support::{
    at, month, month_start, mtime_ms, record, set_mtime_ms, transcript_path, write_records,
};
use super::*;
use crate::paths::scratch_dir;
use crate::usage::files::lock;
use crate::usage::transcripts::USAGE_TRANSCRIPT_PARSER_VERSION;

mod read;
mod watermark;

fn append_record(path: &Path, record: &str) {
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "{record}").unwrap();
}

fn rewrite_same_size_later(path: &Path, record: &str) {
    let (size, mtime) = (std::fs::metadata(path).unwrap().len(), mtime_ms(path));
    write_records(path, &[record.to_string()]);
    set_mtime_ms(path, mtime + 5_000);
    assert_eq!(std::fs::metadata(path).unwrap().len(), size);
}

fn with_sources<R>(
    home: &Path,
    watermark: Watermark,
    use_them: impl FnOnce(&mut Sources) -> R,
) -> (R, SeenSources) {
    with_sources_finished_at(home, watermark, watermark, use_them)
}

fn with_sources_finished_at<R>(
    home: &Path,
    opened_at: Watermark,
    finished_at: Watermark,
    use_them: impl FnOnce(&mut Sources) -> R,
) -> (R, SeenSources) {
    let locked = lock();
    let mut sources = Sources::open(&locked, home, || opened_at);
    let used = use_them(&mut sources);
    (used, sources.finish(|| finished_at))
}

fn read_all(home: &Path, watermark: Watermark) -> SourceRead {
    with_sources(home, watermark, |sources| {
        sources.read(i64::MIN, || watermark)
    })
    .0
}

fn open_and_finish(home: &Path, watermark: Watermark) -> SeenSources {
    with_sources(home, watermark, |_| ()).1
}

fn signature(home: &Path, watermark: Watermark, span: Span) -> String {
    with_sources(home, watermark, |sources| sources.signature(span)).0
}

fn unchanged(seen: &SeenSources) -> bool {
    seen.unchanged(&lock())
}

fn signed_read(home: &Path) -> ((String, SourceRead), SeenSources) {
    with_sources(home, Watermark::NONE, |sources| {
        (
            sources.signature(month(8)),
            sources.read(i64::MIN, || Watermark::NONE),
        )
    })
}

fn claude_present(seen: &SeenSources, read: &SourceRead) -> bool {
    seen.providers(read)
        .find(|source| source.provider == UsageProvider::Claude)
        .unwrap()
        .files
        .is_some()
}

fn index_file(home: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(source_index_path_for(home)).unwrap()).unwrap()
}

#[test]
fn unchanged_sources_keep_their_index_and_window_signature() {
    let home = scratch_dir("usage-sources-unchanged");
    let path = transcript_path(&home, "august.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    let (first_signature, first_seen) = with_sources(&home, Watermark::NONE, |sources| {
        sources.signature(month(8))
    });
    assert_eq!(cached_record_count(&home, &path), Some(1));
    reset_transcript_parse_count();

    let second = signature(&home, Watermark::NONE, month(8));

    assert_eq!(second, first_signature);
    assert_eq!(transcript_parse_count(), 0);
    assert!(unchanged(&first_seen), "the index was not written again");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_created_transcript_changes_the_signature_of_a_window_it_falls_in() {
    let home = scratch_dir("usage-sources-create");
    std::fs::create_dir_all(home.join(".claude").join("projects")).unwrap();
    let (before_signature, before_seen) = with_sources(&home, Watermark::NONE, |sources| {
        sources.signature(month(8))
    });

    let created_path = transcript_path(&home, "created.jsonl");
    write_records(
        &created_path,
        &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)],
    );
    let (created_signature, created_seen) = with_sources(&home, Watermark::NONE, |sources| {
        sources.signature(month(8))
    });
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
    let before = signature(&home, Watermark::NONE, month(8));

    append_record(&path, &record("2026-08-08T04:05:13.944Z", "msg-2", 25));
    let appended = signature(&home, Watermark::NONE, month(8));

    assert_ne!(appended, before);
    assert_eq!(cached_record_count(&home, &path), Some(2));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_same_size_rewrite_with_a_new_mtime_changes_both_windows_it_moves_between() {
    let home = scratch_dir("usage-sources-rewrite");
    let path = transcript_path(&home, "rewrite.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-a", 20)]);
    let august_before = signature(&home, Watermark::NONE, month(8));
    let july_before = signature(&home, Watermark::NONE, month(7));

    rewrite_same_size_later(&path, &record("2026-07-07T04:05:13.944Z", "msg-j", 20));

    assert_ne!(signature(&home, Watermark::NONE, month(8)), august_before);
    assert_ne!(signature(&home, Watermark::NONE, month(7)), july_before);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_deleted_transcript_changes_the_signature_of_its_previous_window() {
    let home = scratch_dir("usage-sources-delete");
    let path = transcript_path(&home, "delete.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    let before = signature(&home, Watermark::NONE, month(8));

    std::fs::remove_file(path).unwrap();

    assert_ne!(signature(&home, Watermark::NONE, month(8)), before);
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
    let july_before = signature(&home, Watermark::NONE, month(7));
    let august_before = signature(&home, Watermark::NONE, month(8));

    append_record(
        &august_path,
        &record("2026-08-08T04:05:13.944Z", "msg-b", 25),
    );

    assert_eq!(signature(&home, Watermark::NONE, month(7)), july_before);
    assert_ne!(signature(&home, Watermark::NONE, month(8)), august_before);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn the_signature_of_no_transcripts_still_depends_on_its_window() {
    let home = scratch_dir("usage-sources-bounds");
    let ((july, august), _) = with_sources(&home, Watermark::NONE, |sources| {
        (sources.signature(month(7)), sources.signature(month(8)))
    });
    assert_ne!(july, august);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_root_appearing_or_going_changes_the_signature_and_the_provider_it_shows() {
    let home = scratch_dir("usage-sources-root-presence");
    let ((missing_signature, missing_read), missing_seen) = signed_read(&home);

    std::fs::create_dir_all(home.join(".claude").join("projects")).unwrap();
    let ((present_signature, present_read), present_seen) = signed_read(&home);

    std::fs::remove_dir_all(home.join(".claude")).unwrap();
    let ((gone_signature, gone_read), gone_seen) = signed_read(&home);

    assert!(!claude_present(&missing_seen, &missing_read));
    assert!(claude_present(&present_seen, &present_read));
    assert!(!claude_present(&gone_seen, &gone_read));
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

    let (complete, _) = with_sources(&home, Watermark::NONE, |sources| sources.is_complete());

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
