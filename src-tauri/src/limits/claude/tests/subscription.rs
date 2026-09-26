//! The subscription status Claude's profile reports rides along with a read and never decides it.

use super::*;

const PROFILE_PAST_DUE: &str = r#"{"account":{"uuid":"uuid-1","email":"me@example.com"},"organization":{"uuid":"org-1","subscription_status":"past_due"}}"#;

#[test]
fn a_claude_read_carries_the_subscription_status_its_profile_reports() {
    let home = scratch_dir("limits-claude-subscription");
    write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (profile_url, profile_request) = serve_once("200 OK", PROFILE_PAST_DUE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    let lookup = read_claude_credential(&StorageDir::default_in(&home), Ok(None), NOW_MS);

    let dto = claude_limits(lookup, &None, &profile_url, &usage_url).dto;
    profile_request.join().unwrap();
    usage_request.join().unwrap();

    assert_eq!(dto.status, LimitsStatus::Ok, "{:?}", dto.message);
    assert_eq!(dto.reading.subscription_status.as_deref(), Some("past_due"));
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn a_profile_without_a_subscription_status_still_reads() {
    let home = scratch_dir("limits-claude-subscription-none");
    write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    let lookup = read_claude_credential(&StorageDir::default_in(&home), Ok(None), NOW_MS);

    let dto = claude_limits(lookup, &None, &profile_url, &usage_url).dto;
    profile_request.join().unwrap();
    usage_request.join().unwrap();

    assert_eq!(dto.status, LimitsStatus::Ok, "{:?}", dto.message);
    assert_eq!(dto.reading.subscription_status, None);
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn the_subscription_status_crosses_to_the_ui_in_camel_case() {
    let mut dto = finish(AgentId::Claude, LimitsStatus::Ok, None, Parsed::default());
    dto.reading.subscription_status = Some("canceled".to_string());

    let value = serde_json::to_value(&dto).unwrap();

    assert_eq!(value["subscriptionStatus"], "canceled");
    dto.reading.subscription_status = None;
    assert!(serde_json::to_value(&dto)
        .unwrap()
        .get("subscriptionStatus")
        .is_none());
}
