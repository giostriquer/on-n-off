use super::*;

#[test]
fn a_wall_clock_gap_larger_than_the_heartbeat_detects_system_wake() {
    let started = std::time::UNIX_EPOCH + Duration::from_secs(1_000);

    assert!(!wake_gap_detected(
        started,
        started + Duration::from_secs(30),
        Duration::from_secs(30),
    ));
    assert!(wake_gap_detected(
        started,
        started + Duration::from_secs(40),
        Duration::from_secs(30),
    ));
    assert!(wake_gap_detected(
        started,
        started - Duration::from_secs(1),
        Duration::from_secs(30),
    ));
}

#[test]
fn backoff_doubles_per_failure_and_stops_at_the_cap() {
    let minute = Duration::from_secs(60);
    assert_eq!(backoff(minute, 0, Duration::from_secs(600)), minute);
    assert_eq!(backoff(minute, 1, Duration::from_secs(600)), minute * 2);
    assert_eq!(
        backoff(minute, 4, Duration::from_secs(600)),
        Duration::from_secs(600)
    );
    assert_eq!(
        backoff(minute, 40, Duration::from_secs(600)),
        Duration::from_secs(600)
    );
}
