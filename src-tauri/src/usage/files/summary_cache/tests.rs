use super::*;
use crate::dto::{UsagePricingDto, UsagePricingStatus, UsageSummaryDto, UsageSummaryInput};
use crate::paths::scratch_dir;
use crate::usage::files::lock;
use crate::usage::window::UsageWindow;

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

fn days(since_day: &str, until_day: &str) -> CacheIdentity {
    identity(UsageSummaryInput {
        since_day: since_day.into(),
        until_day: until_day.into(),
        time_zone: "UTC".into(),
        resolution: Some("day".into()),
        since_time: None,
        until_time: None,
        force: false,
    })
}

fn hours(since: &str, until: &str) -> CacheIdentity {
    identity(UsageSummaryInput {
        resolution: Some("hour".into()),
        since_time: Some(since.into()),
        until_time: Some(until.into()),
        ..days_input()
    })
}

fn days_input() -> UsageSummaryInput {
    UsageSummaryInput {
        since_day: "2026-08-07".into(),
        until_day: "2026-08-08".into(),
        time_zone: "UTC".into(),
        resolution: None,
        since_time: None,
        until_time: None,
        force: false,
    }
}

fn identity(input: UsageSummaryInput) -> CacheIdentity {
    UsageWindow::of(&input).unwrap().cache_identity()
}

#[test]
fn round_trip_hit_requires_matching_source_signature() {
    let home = scratch_dir("usage-summary-cache");
    let path = summary_cache_path_for(&home);
    let key = summary_key(&days("2026-08-01", "2026-08-13"), None, "none");
    let signature = "v1-a";
    store_summary(&lock(), &path, &key, signature, &sample_dto());
    let hit = load_summary_hit(&path, &key, signature).expect("hit");
    assert!(hit.cache_hit);
    assert_eq!(hit.scan_duration_ms, 0);
    assert!(load_summary_hit(&path, &key, "v1-b").is_none());
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn summary_key_changes_with_the_rate_table() {
    let august = days("2026-08-01", "2026-08-31");
    assert_ne!(
        summary_key(&august, Some(1), "none"),
        summary_key(&august, Some(2), "none")
    );
    assert_ne!(
        summary_key(&august, None, "none"),
        summary_key(&august, Some(1), "none")
    );
}

#[test]
fn summary_key_changes_with_the_usage_history() {
    let august = days("2026-08-01", "2026-08-31");
    assert_ne!(
        summary_key(&august, Some(1), "none"),
        summary_key(&august, Some(1), "812:17")
    );
    assert_ne!(
        summary_key(&august, Some(1), "812:17"),
        summary_key(&august, Some(1), "812:18")
    );
}

#[test]
fn a_stored_summary_replaces_the_entries_of_its_group() {
    let home = scratch_dir("usage-summary-cache-groups");
    let path = summary_cache_path_for(&home);
    let august = summary_key(&days("2026-08-01", "2026-08-31"), Some(1), "none");
    let august_newer_rates = summary_key(&days("2026-08-01", "2026-08-31"), Some(2), "none");
    let july = summary_key(&days("2026-07-01", "2026-07-31"), Some(1), "none");
    let earlier_hours = summary_key(
        &hours("2026-08-07T00:00:00Z", "2026-08-08T00:00:00Z"),
        Some(1),
        "none",
    );
    let later_hours = summary_key(
        &hours("2026-08-07T00:01:00Z", "2026-08-08T00:01:00Z"),
        Some(1),
        "none",
    );
    for key in [
        &august,
        &july,
        &earlier_hours,
        &later_hours,
        &august_newer_rates,
    ] {
        store_summary(&lock(), &path, key, "sig", &sample_dto());
    }

    let kept = |key: &SummaryKey| load_summary_hit(&path, key, "sig").is_some();
    assert!(kept(&july));
    assert!(kept(&later_hours));
    assert!(kept(&august_newer_rates));
    assert!(!kept(&earlier_hours));
    assert!(!kept(&august));
    let _ = std::fs::remove_dir_all(home);
}
