use super::test_support::*;
use super::*;
use crate::dto::{UsageCostSource, UsagePricingStatus, UsageSourceStatus};
use crate::paths::scratch_dir;
use crate::usage::pricing;
use crate::usage::scan_cache::{reset_scan_cache_decode_count, scan_cache_decode_count};
use crate::usage::source_index::{reset_transcript_parse_count, transcript_parse_count};

#[test]
fn cached_summary_is_invalidated_when_transcript_is_appended() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-summary-append");
    write_claude_transcript(&home);
    std::env::set_var("ON_N_OFF_HOME", &home);

    let initial = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(!initial.cache_hit);
    assert_eq!(initial.buckets[0].totals.output_tokens, 20);

    append_claude_record(&home, "msg_2", 25);
    let refreshed = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(!refreshed.cache_hit);
    assert_eq!(output_tokens(&refreshed), 45);

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn cached_summary_is_invalidated_when_transcript_is_created() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-summary-create");
    std::fs::create_dir_all(home.join(".claude").join("projects")).unwrap();
    std::env::set_var("ON_N_OFF_HOME", &home);
    let initial = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(initial.buckets.is_empty());

    write_single_claude_record(&home, "created.jsonl", "2026-08-07T04:05:13.944Z", 20);
    let refreshed = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(!refreshed.cache_hit);
    assert_eq!(output_tokens(&refreshed), 20);

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn cached_summary_is_invalidated_by_same_size_rewrite() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-summary-rewrite");
    let path = write_single_claude_record(&home, "rewrite.jsonl", "2026-08-07T04:05:13.944Z", 20);
    std::env::set_var("ON_N_OFF_HOME", &home);
    let initial = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert_eq!(output_tokens(&initial), 20);
    let original_size = std::fs::metadata(&path).unwrap().len();

    std::thread::sleep(std::time::Duration::from_millis(20));
    write_single_claude_record(&home, "rewrite.jsonl", "2026-08-07T04:05:13.944Z", 25);
    assert_eq!(std::fs::metadata(path).unwrap().len(), original_size);
    let refreshed = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(!refreshed.cache_hit);
    assert_eq!(output_tokens(&refreshed), 25);

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn cached_summary_is_invalidated_when_transcript_is_deleted() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-summary-delete");
    let path = write_single_claude_record(&home, "delete.jsonl", "2026-08-07T04:05:13.944Z", 20);
    std::env::set_var("ON_N_OFF_HOME", &home);
    let initial = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert_eq!(output_tokens(&initial), 20);

    std::fs::remove_file(path).unwrap();
    let refreshed = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(!refreshed.cache_hit);
    assert!(refreshed.buckets.is_empty());

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn cached_summary_survives_disjoint_window_change() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-summary-disjoint");
    write_single_claude_record(&home, "july.jsonl", "2026-07-07T04:05:13.944Z", 10);
    let august_path =
        write_single_claude_record(&home, "august.jsonl", "2026-08-07T04:05:13.944Z", 20);
    std::env::set_var("ON_N_OFF_HOME", &home);
    let july_input = || day_input("2026-07-01", "2026-07-31", false);
    let initial = pricing::with_test_fetch(None, || read_summary(july_input())).unwrap();
    assert_eq!(output_tokens(&initial), 10);

    std::thread::sleep(std::time::Duration::from_millis(20));
    let mut august = std::fs::read_to_string(&august_path).unwrap();
    august.push_str(
        &serde_json::json!({
            "type": "assistant",
            "timestamp": "2026-08-08T04:05:13.944Z",
            "sessionId": "sess-claude",
            "message": {
                "id": "august-2",
                "model": "claude-fable-5",
                "usage": { "input_tokens": 1, "output_tokens": 25 }
            }
        })
        .to_string(),
    );
    august.push('\n');
    std::fs::write(august_path, august).unwrap();

    let unchanged = pricing::with_test_fetch(None, || read_summary(july_input())).unwrap();
    assert!(unchanged.cache_hit);
    assert_eq!(output_tokens(&unchanged), 10);

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn corrupt_summary_cache_is_rebuilt() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-summary-corrupt");
    write_claude_transcript(&home);
    std::env::set_var("ON_N_OFF_HOME", &home);
    pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    let summary_path = summary_cache_path_for(&home);
    std::fs::write(&summary_path, "{partial").unwrap();

    let rebuilt = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(!rebuilt.cache_hit);
    assert_eq!(output_tokens(&rebuilt), 20);
    assert!(serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(summary_path).unwrap()
    )
    .is_ok());

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn incompatible_and_partial_cache_documents_rebuild_together() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-summary-cache-migration");
    write_claude_transcript(&home);
    let cache_root = home.join(".on-n-off");
    std::fs::create_dir_all(&cache_root).unwrap();
    std::fs::write(
        cache_root.join("usage-scan-cache.json"),
        r#"{"version":1,"models":[],"sessions":[],"files":{}}"#,
    )
    .unwrap();
    std::fs::write(
        cache_root.join("usage-source-index.json"),
        r#"{"version":1,"generation":99,"entries":{}}"#,
    )
    .unwrap();
    std::fs::write(
        cache_root.join("usage-summary-cache.json"),
        r#"{"version":1,"entries":[]}"#,
    )
    .unwrap();
    std::env::set_var("ON_N_OFF_HOME", &home);

    let migrated = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(!migrated.cache_hit);
    assert_eq!(output_tokens(&migrated), 20);
    for (cache_name, expected_version) in [
        (
            "usage-source-index.json",
            super::super::source_index::USAGE_SOURCE_INDEX_VERSION,
        ),
        (
            "usage-scan-cache.json",
            super::super::scan_cache::USAGE_SCAN_CACHE_VERSION,
        ),
        (
            "usage-summary-cache.json",
            super::super::summary_cache::USAGE_SUMMARY_CACHE_VERSION,
        ),
    ] {
        let document: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(cache_root.join(cache_name)).unwrap())
                .unwrap();
        assert_eq!(
            document["version"].as_u64(),
            Some(u64::from(expected_version))
        );
    }

    for cache_name in [
        "usage-source-index.json",
        "usage-scan-cache.json",
        "usage-summary-cache.json",
    ] {
        std::fs::write(cache_root.join(cache_name), "{partial").unwrap();
    }
    let rebuilt = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(!rebuilt.cache_hit);
    assert_eq!(output_tokens(&rebuilt), 20);

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn older_generation_cannot_overwrite_newer_summary() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-summary-concurrent");
    write_claude_transcript(&home);
    std::env::set_var("ON_N_OFF_HOME", &home);

    let reached = Arc::new(std::sync::Barrier::new(2));
    let resume = Arc::new(std::sync::Barrier::new(2));
    let old_reached = Arc::clone(&reached);
    let old_resume = Arc::clone(&resume);
    let older = std::thread::spawn(move || {
        with_before_publish_pause(old_reached, old_resume, || {
            pricing::with_test_fetch(None, || read_summary(august_input(true))).unwrap()
        })
    });

    reached.wait();
    append_claude_record(&home, "msg_2", 25);
    let newer = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert_eq!(output_tokens(&newer), 45);
    resume.wait();

    let older = older.join().unwrap();
    assert_eq!(output_tokens(&older), 20);
    let final_hit = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(final_hit.cache_hit);
    assert_eq!(output_tokens(&final_hit), 45);
    for cache_name in [
        "usage-source-index.json",
        "usage-scan-cache.json",
        "usage-summary-cache.json",
    ] {
        let raw = std::fs::read_to_string(home.join(".on-n-off").join(cache_name)).unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&raw).is_ok());
    }

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn cache_write_failures_do_not_fail_correct_summary() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-summary-write-failure");
    write_claude_transcript(&home);
    std::fs::write(home.join(".on-n-off"), "blocks cache directory").unwrap();
    std::env::set_var("ON_N_OFF_HOME", &home);

    let summary = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(!summary.cache_hit);
    assert_eq!(output_tokens(&summary), 20);

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn missing_dirs_report_missing_sources() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-missing");
    std::env::set_var("ON_N_OFF_HOME", &home);
    let summary = pricing::with_test_fetch(None, || {
        read_summary(UsageSummaryInput {
            since_day: "2026-08-01".into(),
            until_day: "2026-08-31".into(),
            time_zone: "UTC".into(),
            resolution: Some("day".into()),
            since_time: None,
            until_time: None,
            force: false,
        })
    })
    .unwrap();
    assert_eq!(summary.sources.len(), 2);
    assert!(summary
        .sources
        .iter()
        .all(|s| s.status == UsageSourceStatus::Missing));
    assert!(summary.buckets.is_empty());
    assert_eq!(summary.pricing.status, UsagePricingStatus::Unavailable);
    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn cached_missing_source_tracks_empty_root_creation_and_removal() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-root-presence");
    std::env::set_var("ON_N_OFF_HOME", &home);

    let missing = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert_eq!(
        missing
            .sources
            .iter()
            .find(|source| source.provider == AgentId::Claude)
            .unwrap()
            .status,
        UsageSourceStatus::Missing
    );
    let cached_missing =
        pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(cached_missing.cache_hit);

    std::fs::create_dir_all(home.join(".claude").join("projects")).unwrap();
    let present = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(!present.cache_hit);
    assert_eq!(
        present
            .sources
            .iter()
            .find(|source| source.provider == AgentId::Claude)
            .unwrap()
            .status,
        UsageSourceStatus::Ok
    );

    std::fs::remove_dir_all(home.join(".claude")).unwrap();
    let missing_again =
        pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(!missing_again.cache_hit);
    assert_eq!(
        missing_again
            .sources
            .iter()
            .find(|source| source.provider == AgentId::Claude)
            .unwrap()
            .status,
        UsageSourceStatus::Missing
    );

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn retained_history_repeated_full_time_reparses_zero_unchanged_files() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-retained-history-repeat");
    let old_path = write_single_claude_record(&home, "old.jsonl", "2025-01-07T04:05:13.944Z", 10);
    age_file(&old_path, 180);
    write_single_claude_record(&home, "recent.jsonl", "2026-08-07T04:05:13.944Z", 20);
    std::env::set_var("ON_N_OFF_HOME", &home);

    let initial = pricing::with_test_fetch(None, || read_summary(full_time_input(true))).unwrap();
    assert_eq!(output_tokens(&initial), 30);
    reset_transcript_parse_count();

    let repeated = pricing::with_test_fetch(None, || read_summary(full_time_input(true))).unwrap();
    assert_eq!(output_tokens(&repeated), 30);
    assert_eq!(transcript_parse_count(), 0);

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn retained_history_unchanged_summary_hit_decodes_zero_scan_cache_records() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-retained-history-fast-hit");
    let old_path = write_single_claude_record(&home, "old.jsonl", "2025-01-07T04:05:13.944Z", 10);
    age_file(&old_path, 180);
    write_single_claude_record(&home, "recent.jsonl", "2026-08-07T04:05:13.944Z", 20);
    std::env::set_var("ON_N_OFF_HOME", &home);

    let initial = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(!initial.cache_hit);
    reset_scan_cache_decode_count();

    let cached = pricing::with_test_fetch(None, || read_summary(august_input(false))).unwrap();
    assert!(cached.cache_hit);
    assert_eq!(scan_cache_decode_count(), 0);

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn retained_history_recent_window_keeps_old_live_cached_records() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-retained-history-recent");
    let old_path = write_single_claude_record(&home, "old.jsonl", "2025-01-07T04:05:13.944Z", 10);
    age_file(&old_path, 180);
    write_single_claude_record(&home, "recent.jsonl", "2026-08-07T04:05:13.944Z", 20);
    std::env::set_var("ON_N_OFF_HOME", &home);

    pricing::with_test_fetch(None, || read_summary(full_time_input(true))).unwrap();
    let recent = pricing::with_test_fetch(None, || read_summary(august_input(true))).unwrap();
    assert_eq!(output_tokens(&recent), 20);
    let cache = load_scan_cache(&scan_cache_path().unwrap());
    assert!(cache.contains_key(&normalize_path(&old_path)));
    reset_transcript_parse_count();

    let full_time = pricing::with_test_fetch(None, || read_summary(full_time_input(true))).unwrap();
    assert_eq!(output_tokens(&full_time), 30);
    assert_eq!(transcript_parse_count(), 0);

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn retained_history_deleted_historical_file_is_pruned() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-retained-history-delete");
    let old_path = write_single_claude_record(&home, "old.jsonl", "2025-01-07T04:05:13.944Z", 10);
    age_file(&old_path, 180);
    write_single_claude_record(&home, "recent.jsonl", "2026-08-07T04:05:13.944Z", 20);
    std::env::set_var("ON_N_OFF_HOME", &home);

    pricing::with_test_fetch(None, || read_summary(full_time_input(true))).unwrap();
    std::fs::remove_file(&old_path).unwrap();
    pricing::with_test_fetch(None, || read_summary(august_input(true))).unwrap();
    let cache = load_scan_cache(&scan_cache_path().unwrap());
    assert!(!cache.contains_key(&normalize_path(&old_path)));

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn retained_history_mixed_age_totals_equal_from_scratch_scan() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-retained-history-equality");
    let old_path = write_single_claude_record(&home, "old.jsonl", "2025-01-07T04:05:13.944Z", 10);
    age_file(&old_path, 180);
    write_single_claude_record(&home, "recent.jsonl", "2026-08-07T04:05:13.944Z", 20);
    std::env::set_var("ON_N_OFF_HOME", &home);

    let warm = pricing::with_test_fetch(None, || read_summary(full_time_input(true))).unwrap();
    assert_eq!(output_tokens(&warm), 30);
    assert_eq!(record_count(&warm), 2);
    let warm_cache = load_scan_cache(&scan_cache_path().unwrap());
    assert!(warm_cache.contains_key(&normalize_path(&old_path)));
    for cache_name in [
        "usage-source-index.json",
        "usage-scan-cache.json",
        "usage-summary-cache.json",
    ] {
        let _ = std::fs::remove_file(home.join(".on-n-off").join(cache_name));
    }

    let from_scratch =
        pricing::with_test_fetch(None, || read_summary(full_time_input(true))).unwrap();
    assert_eq!(output_tokens(&from_scratch), 30);
    assert_eq!(record_count(&from_scratch), 2);
    assert_eq!(from_scratch.buckets, warm.buckets);

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn scans_claude_fixture_and_dedupes() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-scan");
    write_claude_transcript(&home);
    std::env::set_var("ON_N_OFF_HOME", &home);
    let rates_doc = serde_json::json!({
        "claude-fable-5": {
            "input_cost_per_token": 1e-5,
            "output_cost_per_token": 5e-5,
            "cache_read_input_token_cost": 1e-6,
            "cache_creation_input_token_cost": 1.25e-5
        }
    });
    let summary = pricing::with_test_fetch(Some(rates_doc), || {
        read_summary(UsageSummaryInput {
            since_day: "2026-08-01".into(),
            until_day: "2026-08-31".into(),
            time_zone: "UTC".into(),
            resolution: Some("day".into()),
            since_time: None,
            until_time: None,
            force: false,
        })
    })
    .unwrap();
    let claude = summary
        .sources
        .iter()
        .find(|s| s.provider == AgentId::Claude)
        .unwrap();
    assert_eq!(claude.status, UsageSourceStatus::Ok);
    assert_eq!(claude.scanned_files, 1);
    assert_eq!(summary.buckets.len(), 1);
    assert_eq!(summary.buckets[0].records, 1);
    assert_eq!(summary.buckets[0].totals.output_tokens, 20);
    assert_eq!(summary.buckets[0].cost_source, UsageCostSource::ModelPriced);
    let expected = 10.0 * 1e-5 + 20.0 * 5e-5;
    assert!((summary.buckets[0].cost_usd - expected).abs() < 1e-12);
    assert_eq!(summary.pricing.status, UsagePricingStatus::Fresh);
    assert!(summary.pricing.known_models >= 1);

    let again = pricing::with_test_fetch(None, || {
        read_summary(UsageSummaryInput {
            since_day: "2026-08-01".into(),
            until_day: "2026-08-31".into(),
            time_zone: "UTC".into(),
            resolution: Some("day".into()),
            since_time: None,
            until_time: None,
            force: true,
        })
    })
    .unwrap();
    assert_eq!(again.buckets[0].totals.output_tokens, 20);
    assert_eq!(again.pricing.status, UsagePricingStatus::Cached);
    assert!(!again.cache_hit);
    assert!(home
        .join(".on-n-off")
        .join("usage-scan-cache.json")
        .is_file());

    let cached = pricing::with_test_fetch(None, || {
        read_summary(UsageSummaryInput {
            since_day: "2026-08-01".into(),
            until_day: "2026-08-31".into(),
            time_zone: "UTC".into(),
            resolution: Some("day".into()),
            since_time: None,
            until_time: None,
            force: false,
        })
    })
    .unwrap();
    assert!(cached.cache_hit);
    assert_eq!(cached.scan_duration_ms, 0);
    assert_eq!(cached.buckets[0].totals.output_tokens, 20);

    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn tokens_still_returned_when_rates_unavailable() {
    let _guard = env_lock().lock().unwrap();
    let home = scratch_dir("usage-unpriced");
    write_claude_transcript(&home);
    std::env::set_var("ON_N_OFF_HOME", &home);
    let summary = pricing::with_test_fetch(None, || {
        read_summary(UsageSummaryInput {
            since_day: "2026-08-01".into(),
            until_day: "2026-08-31".into(),
            time_zone: "UTC".into(),
            resolution: Some("day".into()),
            since_time: None,
            until_time: None,
            force: false,
        })
    })
    .unwrap();
    assert_eq!(summary.pricing.status, UsagePricingStatus::Unavailable);
    assert_eq!(summary.buckets[0].cost_source, UsageCostSource::Unpriced);
    assert_eq!(summary.buckets[0].totals.output_tokens, 20);
    std::env::remove_var("ON_N_OFF_HOME");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn invalid_window_errors() {
    let err = read_summary(UsageSummaryInput {
        since_day: "2026-08-10".into(),
        until_day: "2026-08-01".into(),
        time_zone: "UTC".into(),
        resolution: None,
        since_time: None,
        until_time: None,
        force: false,
    })
    .unwrap_err();
    assert!(err.message.contains("after untilDay"));
}

/// Claim-check harness: time real-home common windows and reusable Full time.
/// `cargo test --release --manifest-path src-tauri/Cargo.toml bench_real_home_usage_summary -- --ignored --nocapture`
#[test]
#[ignore = "real-home performance probe; not part of CI"]
fn bench_real_home_usage_summary() {
    let until = chrono::Local::now().date_naive();
    let mut output = Vec::new();
    let windows = [("7d", 7), ("30d", 30), ("90d", 90)];
    for (label, days) in windows {
        let since = until - chrono::Duration::days(days - 1);
        let input = UsageSummaryInput {
            since_day: since.format("%Y-%m-%d").to_string(),
            until_day: until.format("%Y-%m-%d").to_string(),
            time_zone: "America/Sao_Paulo".into(),
            resolution: Some("day".into()),
            since_time: None,
            until_time: None,
            force: false,
        };
        output.push(format!(
            "bench window={label} {} .. {}",
            input.since_day, input.until_day
        ));
        // A live agent may update a source during the forced read, which correctly prevents
        // publication. Report the follow-up result instead of requiring a cache hit.
        let measurements = [("forced", true), ("cache-attempt", false)].map(|(pass, force)| {
            let wall = Instant::now();
            let dto = read_summary(UsageSummaryInput {
                force,
                ..input.clone()
            })
            .expect("read_summary");
            (pass, force, wall.elapsed().as_millis(), dto)
        });
        for (pass, force, wall_ms, dto) in measurements {
            let scanned: u64 = dto.sources.iter().map(|source| source.scanned_files).sum();
            let skipped: u64 = dto.sources.iter().map(|source| source.skipped_files).sum();
            let sessions: u64 = dto
                .sources
                .iter()
                .map(|source| source.distinct_sessions)
                .sum();
            output.push(format!(
                "window={label} pass={pass} wall_ms={wall_ms} scan_duration_ms={} cache_hit={} buckets={} scanned_files={} skipped_files={} sessions={sessions}",
                dto.scan_duration_ms,
                dto.cache_hit,
                dto.buckets.len(),
                scanned,
                skipped,
            ));
            if force {
                assert!(!dto.cache_hit);
            }
        }
    }

    let full_time = UsageSummaryInput {
        since_day: "2020-01-01".into(),
        until_day: until.format("%Y-%m-%d").to_string(),
        time_zone: "America/Sao_Paulo".into(),
        resolution: Some("day".into()),
        since_time: None,
        until_time: None,
        force: true,
    };
    let measurements = (1..=2)
        .map(|pass| {
            let wall = Instant::now();
            let dto = read_summary(full_time.clone()).expect("read_summary");
            (pass, wall.elapsed().as_millis(), dto)
        })
        .collect::<Vec<_>>();
    for (pass, wall_ms, dto) in measurements {
        let scanned: u64 = dto.sources.iter().map(|source| source.scanned_files).sum();
        let skipped: u64 = dto.sources.iter().map(|source| source.skipped_files).sum();
        let sessions: u64 = dto
            .sources
            .iter()
            .map(|source| source.distinct_sessions)
            .sum();
        output.push(format!(
            "window=full pass={pass} wall_ms={wall_ms} scan_duration_ms={} cache_hit={} buckets={} scanned_files={} skipped_files={} sessions={sessions}",
            dto.scan_duration_ms,
            dto.cache_hit,
            dto.buckets.len(),
            scanned,
            skipped,
        ));
        assert!(!dto.cache_hit);
    }
    for line in output {
        eprintln!("{line}");
    }
}
