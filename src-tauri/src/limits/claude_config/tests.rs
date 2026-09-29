//! The account a Claude config dir's `.claude.json` names, and the plan its organization is on.
use super::*;
use crate::paths::scratch_dir;
use std::fs;

fn write(home: &Path, rel: &str, body: &str) {
    let path = home.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn the_account_comes_from_the_config_files_oauth_account() {
    let home = scratch_dir("limits-cred");
    let file = home.join(".claude.json");
    assert!(read_claude_config_account(&file).is_none());
    write(
        &home,
        ".claude.json",
        r#"{"oauthAccount":{"accountUuid":"uuid-1","emailAddress":"me@example.com","organizationName":"Org","organizationUuid":"org-1","organizationType":"claude_max","organizationRateLimitTier":"default_claude_max_5x"},"userID":"x"}"#,
    );
    let account = read_claude_config_account(&file).expect("the account");
    assert_eq!(
        account.identity,
        ClaudeIdentity {
            account: LimitsAccountDto {
                legacy_id: None,
                id: "uuid-1".to_string(),
                label: Some("me@example.com".to_string())
            },
            organization_id: Some("org-1".to_string())
        }
    );
    assert_eq!(account.plan.as_deref(), Some("max ×5"));
    write(
        &home,
        ".claude.json",
        r#"{"oauthAccount":{"emailAddress":"no-uuid@example.com"}}"#,
    );
    assert!(read_claude_config_account(&file).is_none());
    write(&home, ".claude.json", "{broken");
    assert!(read_claude_config_account(&file).is_none());
}

#[test]
fn an_unrecognized_tier_keeps_the_plan_as_its_type_says() {
    let home = scratch_dir("limits-cred-plan");
    write(
        &home,
        ".claude.json",
        r#"{"oauthAccount":{"accountUuid":"uuid-1","organizationType":"claude_pro","organizationRateLimitTier":"default_claude_ai"}}"#,
    );

    let account = read_claude_config_account(&home.join(".claude.json")).expect("the account");

    assert_eq!(account.plan.as_deref(), Some("pro"));
}
