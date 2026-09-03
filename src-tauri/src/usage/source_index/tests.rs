use super::*;
use crate::paths::scratch_dir;
use std::thread;
use std::time::Duration;

const JULY_START: i64 = 1_783_036_800_000;
const AUGUST_START: i64 = 1_785_715_200_000;
const SEPTEMBER_START: i64 = 1_788_393_600_000;

fn roots(home: &Path) -> Vec<SourceRoot> {
    vec![SourceRoot {
        provider: UsageProvider::Claude,
        path: home.join(".claude").join("projects"),
    }]
}

fn transcript_path(home: &Path, name: &str) -> PathBuf {
    home.join(".claude")
        .join("projects")
        .join("fixture")
        .join(name)
}

fn record(timestamp: &str, message_id: &str, output_tokens: u64) -> String {
    serde_json::json!({
        "type": "assistant",
        "timestamp": timestamp,
        "sessionId": "source-index-session",
        "message": {
            "id": message_id,
            "model": "claude-fable-5",
            "usage": {
                "input_tokens": 1,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0,
                "output_tokens": output_tokens
            }
        }
    })
    .to_string()
}

fn write_records(path: &Path, records: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("{}\n", records.join("\n"))).unwrap();
}

fn reconcile_home(home: &Path, cache: &mut ScanCache) -> ReconcileOutcome {
    reconcile(&source_index_path_for(home), &roots(home), cache)
}

#[test]
fn unchanged_sources_keep_generation_and_window_signature() {
    let home = scratch_dir("usage-source-index-unchanged");
    write_records(
        &transcript_path(&home, "august.jsonl"),
        &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)],
    );
    let mut cache = ScanCache::new();
    let first = reconcile_home(&home, &mut cache);
    let first_signature = first.snapshot.signature(AUGUST_START, SEPTEMBER_START);
    assert!(first.scan_cache_dirty);
    assert!(first
        .snapshot
        .persisted_generation_is_current(&source_index_path_for(&home)));

    let second = reconcile_home(&home, &mut cache);
    assert!(!second.scan_cache_dirty);
    assert_eq!(second.snapshot.generation(), first.snapshot.generation());
    assert_eq!(
        second.snapshot.signature(AUGUST_START, SEPTEMBER_START),
        first_signature
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn created_source_changes_intersecting_window_signature() {
    let home = scratch_dir("usage-source-index-create");
    let mut cache = ScanCache::new();
    let empty = reconcile_home(&home, &mut cache);
    let before = empty.snapshot.signature(AUGUST_START, SEPTEMBER_START);

    write_records(
        &transcript_path(&home, "created.jsonl"),
        &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)],
    );
    let created = reconcile_home(&home, &mut cache);
    assert!(!empty
        .snapshot
        .persisted_generation_is_current(&source_index_path_for(&home)));
    assert!(created
        .snapshot
        .persisted_generation_is_current(&source_index_path_for(&home)));
    assert_ne!(
        created.snapshot.signature(AUGUST_START, SEPTEMBER_START),
        before
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn appended_source_changes_intersecting_window_signature() {
    let home = scratch_dir("usage-source-index-append");
    let path = transcript_path(&home, "append.jsonl");
    let first_record = record("2026-08-07T04:05:13.944Z", "msg-1", 20);
    write_records(&path, std::slice::from_ref(&first_record));
    let mut cache = ScanCache::new();
    let first = reconcile_home(&home, &mut cache);
    let before = first.snapshot.signature(AUGUST_START, SEPTEMBER_START);

    write_records(
        &path,
        &[
            first_record,
            record("2026-08-08T04:05:13.944Z", "msg-2", 25),
        ],
    );
    let appended = reconcile_home(&home, &mut cache);
    assert_ne!(
        appended.snapshot.signature(AUGUST_START, SEPTEMBER_START),
        before
    );
    assert_eq!(cache.get(&normalize_path(&path)).unwrap().records.len(), 2);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn same_size_rewrite_with_new_mtime_changes_time_spans() {
    let home = scratch_dir("usage-source-index-rewrite");
    let path = transcript_path(&home, "rewrite.jsonl");
    let august = record("2026-08-07T04:05:13.944Z", "msg-a", 20);
    let july = record("2026-07-07T04:05:13.944Z", "msg-j", 20);
    assert_eq!(august.len(), july.len());
    write_records(&path, &[august]);
    let mut cache = ScanCache::new();
    let first = reconcile_home(&home, &mut cache);
    let august_before = first.snapshot.signature(AUGUST_START, SEPTEMBER_START);
    let july_before = first.snapshot.signature(JULY_START, AUGUST_START);
    let old_mtime = file_identity(&path).unwrap().1;

    thread::sleep(Duration::from_millis(20));
    write_records(&path, &[july]);
    if file_identity(&path).unwrap().1 == old_mtime {
        thread::sleep(Duration::from_millis(20));
        let same = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, same).unwrap();
    }
    let rewritten = reconcile_home(&home, &mut cache);
    assert_ne!(
        rewritten.snapshot.signature(AUGUST_START, SEPTEMBER_START),
        august_before
    );
    assert_ne!(
        rewritten.snapshot.signature(JULY_START, AUGUST_START),
        july_before
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn deleted_source_changes_its_previous_window_signature() {
    let home = scratch_dir("usage-source-index-delete");
    let path = transcript_path(&home, "delete.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    let mut cache = ScanCache::new();
    let first = reconcile_home(&home, &mut cache);
    let before = first.snapshot.signature(AUGUST_START, SEPTEMBER_START);

    std::fs::remove_file(path).unwrap();
    let deleted = reconcile_home(&home, &mut cache);
    assert_ne!(
        deleted.snapshot.signature(AUGUST_START, SEPTEMBER_START),
        before
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn disjoint_window_signature_ignores_unrelated_source_change() {
    let home = scratch_dir("usage-source-index-disjoint");
    let july_path = transcript_path(&home, "july.jsonl");
    let august_path = transcript_path(&home, "august.jsonl");
    write_records(
        &july_path,
        &[record("2026-07-07T04:05:13.944Z", "msg-j", 10)],
    );
    write_records(
        &august_path,
        &[record("2026-08-07T04:05:13.944Z", "msg-a", 20)],
    );
    let mut cache = ScanCache::new();
    let first = reconcile_home(&home, &mut cache);
    let july_before = first.snapshot.signature(JULY_START, AUGUST_START);
    let august_before = first.snapshot.signature(AUGUST_START, SEPTEMBER_START);

    write_records(
        &august_path,
        &[
            record("2026-08-07T04:05:13.944Z", "msg-a", 20),
            record("2026-08-08T04:05:13.944Z", "msg-b", 25),
        ],
    );
    let changed = reconcile_home(&home, &mut cache);
    assert_eq!(
        changed.snapshot.signature(JULY_START, AUGUST_START),
        july_before
    );
    assert_ne!(
        changed.snapshot.signature(AUGUST_START, SEPTEMBER_START),
        august_before
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn empty_source_signature_includes_requested_bounds() {
    let home = scratch_dir("usage-source-index-bounds");
    let mut cache = ScanCache::new();
    let snapshot = reconcile_home(&home, &mut cache).snapshot;
    assert_ne!(
        snapshot.signature(JULY_START, AUGUST_START),
        snapshot.signature(AUGUST_START, SEPTEMBER_START)
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn empty_root_presence_changes_signature_in_both_directions() {
    let home = scratch_dir("usage-source-index-root-presence");
    let root = roots(&home).remove(0);
    let mut cache = ScanCache::new();

    let missing = reconcile_home(&home, &mut cache);
    let missing_signature = missing.snapshot.signature(AUGUST_START, SEPTEMBER_START);
    assert!(!missing.snapshot.root_is_present(&root));

    std::fs::create_dir_all(&root.path).unwrap();
    let present = reconcile_home(&home, &mut cache);
    let present_signature = present.snapshot.signature(AUGUST_START, SEPTEMBER_START);
    assert!(present.snapshot.root_is_present(&root));
    assert_ne!(present_signature, missing_signature);

    std::fs::remove_dir_all(home.join(".claude")).unwrap();
    let missing_again = reconcile_home(&home, &mut cache);
    assert!(!missing_again.snapshot.root_is_present(&root));
    assert_ne!(
        missing_again
            .snapshot
            .signature(AUGUST_START, SEPTEMBER_START),
        present_signature
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn incomplete_root_walk_retains_previous_entries_and_disables_hits() {
    let home = scratch_dir("usage-source-index-failed-walk");
    write_records(
        &transcript_path(&home, "retained.jsonl"),
        &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)],
    );
    let mut cache = ScanCache::new();
    let first = reconcile_home(&home, &mut cache);
    let generation = first.snapshot.generation();

    let root = roots(&home).remove(0);
    let failed = reconcile_inventories(
        &source_index_path_for(&home),
        vec![(
            root,
            RootInventory {
                files: Vec::new(),
                present: true,
                complete: false,
            },
        )],
        &mut cache,
    );
    assert!(!failed.snapshot.is_complete());
    assert_eq!(failed.snapshot.generation(), generation);
    assert_eq!(failed.snapshot.entries.len(), 1);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn unresolved_file_is_pending_and_retried_on_next_reconciliation() {
    let home = scratch_dir("usage-source-index-pending");
    let path = transcript_path(&home, "pending.jsonl");
    let root = roots(&home).remove(0);
    let observed = TranscriptFile {
        path: path.clone(),
        size: 1,
        mtime_ms: 1,
    };
    let mut cache = ScanCache::new();
    let pending = reconcile_inventories(
        &source_index_path_for(&home),
        vec![(
            root,
            RootInventory {
                files: vec![observed],
                present: true,
                complete: true,
            },
        )],
        &mut cache,
    );
    assert!(!pending.snapshot.is_complete());
    assert!(cache.is_empty());

    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    let resolved = reconcile_home(&home, &mut cache);
    assert!(resolved.snapshot.is_complete());
    assert_eq!(cache.get(&normalize_path(&path)).unwrap().records.len(), 1);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn file_that_changes_during_both_parse_attempts_remains_pending() {
    let identities = std::cell::RefCell::new(vec![(2, 2), (3, 3)].into_iter());
    let result =
        read_stable_records_with(1, 1, || Some(Vec::new()), || identities.borrow_mut().next());
    assert!(result.is_none());
}

#[test]
fn incompatible_v1_index_is_rebuilt() {
    let home = scratch_dir("usage-source-index-v1");
    write_records(
        &transcript_path(&home, "valid.jsonl"),
        &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)],
    );
    let index_path = source_index_path_for(&home);
    std::fs::create_dir_all(index_path.parent().unwrap()).unwrap();
    std::fs::write(&index_path, r#"{"version":1,"generation":99,"entries":{}}"#).unwrap();
    let mut cache = ScanCache::new();
    let rebuilt = reconcile_home(&home, &mut cache);
    assert!(rebuilt.snapshot.is_complete());
    assert_eq!(rebuilt.snapshot.generation(), 1);
    let persisted: SourceIndexFile =
        serde_json::from_str(&std::fs::read_to_string(index_path).unwrap()).unwrap();
    assert_eq!(persisted.version, USAGE_SOURCE_INDEX_VERSION);
    assert_eq!(persisted.parser_version, USAGE_TRANSCRIPT_PARSER_VERSION);
    assert_eq!(persisted.scan_cache_version, USAGE_SCAN_CACHE_VERSION);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn corrupt_index_is_rebuilt_from_disposable_sources() {
    let home = scratch_dir("usage-source-index-corrupt");
    let path = transcript_path(&home, "valid.jsonl");
    write_records(&path, &[record("2026-08-07T04:05:13.944Z", "msg-1", 20)]);
    let index_path = source_index_path_for(&home);
    std::fs::create_dir_all(index_path.parent().unwrap()).unwrap();
    std::fs::write(&index_path, "{partial").unwrap();

    let mut cache = ScanCache::new();
    let rebuilt = reconcile_home(&home, &mut cache);
    assert_eq!(rebuilt.snapshot.generation(), 1);
    assert_eq!(cache.len(), 1);
    let persisted: SourceIndexFile =
        serde_json::from_str(&std::fs::read_to_string(index_path).unwrap()).unwrap();
    assert_eq!(persisted.version, USAGE_SOURCE_INDEX_VERSION);
    assert_eq!(persisted.entries.len(), 1);
    let _ = std::fs::remove_dir_all(home);
}
