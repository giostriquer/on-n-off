use super::*;
use crate::dto::{UsagePricingDto, UsagePricingStatus, UsageSummaryDto, UsageSummaryInput};
use crate::paths::scratch_dir;

fn sample_dto() -> UsageSummaryDto {
    UsageSummaryDto {
        read_at: "2026-08-13T00:00:00Z".into(),
        time_zone: "UTC".into(),
        since_day: "2026-08-01".into(),
        until_day: "2026-08-13".into(),
        buckets: vec![],
        sources: vec![],
        pricing: UsagePricingDto {
            status: UsagePricingStatus::Unavailable,
            source: "none".into(),
            fetched_at: None,
            known_models: 0,
        },
        scan_duration_ms: 42,
        cache_hit: false,
    }
}

#[test]
fn round_trip_hit_requires_matching_source_signature() {
    let home = scratch_dir("usage-summary-cache");
    let path = summary_cache_path_for(&home);
    let input = UsageSummaryInput {
        since_day: "2026-08-01".into(),
        until_day: "2026-08-13".into(),
        time_zone: "UTC".into(),
        resolution: Some("day".into()),
        since_time: None,
        until_time: None,
        force: false,
    };
    let key = window_key(&input);
    let signature = "v1-a";
    store_summary(&path, &key, signature, &sample_dto());
    let hit = load_summary_hit(&path, &key, signature).expect("hit");
    assert!(hit.cache_hit);
    assert_eq!(hit.scan_duration_ms, 0);
    assert!(load_summary_hit(&path, &key, "v1-b").is_none());
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn summary_key_changes_with_the_rate_table() {
    let input = UsageSummaryInput {
        since_day: "2026-08-01".into(),
        until_day: "2026-08-31".into(),
        time_zone: "UTC".into(),
        resolution: None,
        since_time: None,
        until_time: None,
        force: false,
    };
    assert_ne!(summary_key(&input, Some(1)), summary_key(&input, Some(2)));
    assert_ne!(summary_key(&input, None), summary_key(&input, Some(1)));
    assert!(summary_key(&input, Some(7)).starts_with(&window_key(&input)));
}

#[test]
fn window_key_includes_resolution_and_bounds() {
    let input = UsageSummaryInput {
        since_day: "a".into(),
        until_day: "b".into(),
        time_zone: "UTC".into(),
        resolution: Some("hour".into()),
        since_time: Some("t0".into()),
        until_time: Some("t1".into()),
        force: false,
    };
    assert_eq!(window_key(&input), "hour|a|b|UTC|t0|t1");
}
