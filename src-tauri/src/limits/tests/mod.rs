mod claude_observation;
mod memory;

use super::*;
use crate::dto::LimitWindowKind;
use crate::http::{refused_url, serve_once, serve_sequence, HttpError};
use crate::paths::scratch_dir;
use json::window;
use serde_json::json;
use std::cell::Cell;
use std::fs;

#[test]
fn provider_read_guards_serialize_only_the_same_provider() {
    let codex = provider_read_guard(AgentId::Codex);
    assert!(provider_read_lock(AgentId::Codex).try_lock().is_err());
    assert!(!std::ptr::eq(
        provider_read_lock(AgentId::Codex),
        provider_read_lock(AgentId::Claude)
    ));
    drop(codex);
    assert!(provider_read_lock(AgentId::Codex).try_lock().is_ok());
}

fn parsed(windows: Vec<LimitWindowDto>) -> Parsed {
    Parsed {
        account: None,
        plan: Some("max".to_string()),
        windows,
        credits: None,
    }
}

fn account(id: &str, label: &str) -> LimitsAccountDto {
    LimitsAccountDto {
        id: id.to_string(),
        label: Some(label.to_string()),
    }
}

fn write(home: &Path, rel: &str, body: &str) {
    let path = home.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

const CLAUDE_CREDENTIALS: &str = r#"{"claudeAiOauth":{"accessToken":"kc-token","expiresAt":1787022473402,"subscriptionType":"max"}}"#;
/// The same login as `CLAUDE_CREDENTIALS` with the refresh token Claude Code actually stores:
/// the access token lasts 8 hours, the refresh token more than a week.
const CLAUDE_RENEWABLE_CREDENTIALS: &str = r#"{"claudeAiOauth":{"accessToken":"kc-token","expiresAt":1787022473402,"refreshToken":"rt","refreshTokenExpiresAt":1787981634215,"subscriptionType":"max"}}"#;
/// Well before the Claude fixture's `expiresAt`.
const NOW_MS: i64 = 1787000000000;

#[test]
fn missing_login_is_signed_out_and_names_the_cli() {
    let dto = resolve::<()>(AgentId::Claude, None, CredentialLookup::Missing, |_| {
        unreachable!("no fetch without a login")
    });
    assert_eq!(dto.provider, AgentId::Claude);
    assert_eq!(dto.status, LimitsStatus::SignedOut);
    assert!(dto.message.as_deref().unwrap().contains("`claude`"));
    assert!(dto.windows.is_empty());
}

#[test]
fn expired_and_unreadable_logins_map_to_their_statuses_without_loading() {
    let loaded = Cell::new(false);
    // Captures only `&loaded`, so the closure is `Copy` and can be handed to each call.
    let load = |_: &&str| {
        loaded.set(true);
        Ok(Parsed::default())
    };

    let expired = resolve(
        AgentId::Codex,
        None,
        CredentialLookup::Expired { renewable: false },
        load,
    );
    assert_eq!(expired.status, LimitsStatus::Unauthenticated);
    assert!(expired.message.as_deref().unwrap().contains("`codex`"));

    let unreadable = resolve(
        AgentId::Claude,
        Some(account("uuid-1", "me@example.com")),
        CredentialLookup::Unreadable("Keychain denied".to_string()),
        load,
    );
    assert_eq!(unreadable.status, LimitsStatus::Failed);
    assert!(unreadable
        .message
        .as_deref()
        .unwrap()
        .contains("Keychain denied"));
    assert_eq!(
        unreadable.account,
        Some(account("uuid-1", "me@example.com")),
        "a failure still says which account it is about"
    );

    assert!(!loaded.get());
}

#[test]
fn a_rejected_token_is_unauthenticated_and_other_http_failures_are_failed() {
    let rejected = resolve(
        AgentId::Claude,
        None,
        CredentialLookup::Found("token"),
        |_| Err(HttpError::Unauthorized),
    );
    assert_eq!(rejected.status, LimitsStatus::Unauthenticated);

    let offline = resolve(
        AgentId::Claude,
        None,
        CredentialLookup::Found("token"),
        |_| Err(HttpError::Network("dns".to_string())),
    );
    assert_eq!(offline.status, LimitsStatus::Failed);
    assert!(offline.message.as_deref().unwrap().contains("dns"));

    let server = resolve(
        AgentId::Claude,
        None,
        CredentialLookup::Found("token"),
        |_| Err(HttpError::Status(503)),
    );
    assert_eq!(server.status, LimitsStatus::Failed);
    assert!(server.message.as_deref().unwrap().contains("503"));
}

#[test]
fn a_successful_load_is_ok_with_windows_ordered_weekly_session_model() {
    let dto = resolve(
        AgentId::Claude,
        None,
        CredentialLookup::Found("token"),
        |token| {
            assert_eq!(*token, "token");
            Ok(parsed(vec![
                window("m", "Weekly · Opus", LimitWindowKind::Model, 3.0, None),
                window(
                    "s",
                    "5 hour · all models",
                    LimitWindowKind::Session,
                    7.0,
                    None,
                ),
                window(
                    "w",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    12.0,
                    None,
                ),
                window("m2", "Weekly · Sonnet", LimitWindowKind::Model, 1.0, None),
            ]))
        },
    );
    assert_eq!(dto.status, LimitsStatus::Ok);
    assert_eq!(dto.message, None);
    assert_eq!(dto.plan.as_deref(), Some("max"));
    let ids: Vec<&str> = dto.windows.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["w", "s", "m", "m2"]);
    assert!(dto
        .windows
        .iter()
        .all(|window| { chrono::DateTime::parse_from_rfc3339(&window.observed_at).is_ok() }));
}

/// Test doubles for `read_limits_in`: a scratch home, a fresh memo, a counting Keychain probe.
struct Rig {
    home: std::path::PathBuf,
    memo: ClaudeLoginMemo,
    probes: Cell<u32>,
    keychain_json: Option<String>,
}

impl Rig {
    fn new(prefix: &str) -> Self {
        Self {
            home: scratch_dir(prefix),
            memo: ClaudeLoginMemo::new(),
            probes: Cell::new(0),
            keychain_json: None,
        }
    }

    fn read(
        &self,
        agent: AgentId,
        force: bool,
        claude_profile_url: &str,
        claude_url: &str,
    ) -> Vec<ProviderLimitsDto> {
        read_limits_in(
            agent,
            force,
            Sources {
                home: &self.home,
                memo: &self.memo,
                keychain: || {
                    self.probes.set(self.probes.get() + 1);
                    Ok(self.keychain_json.clone())
                },
                claude_profile_url,
                claude_url,
                claude_desktop_history: claude_desktop::history_path_for_home(&self.home),
                now_ms: NOW_MS,
            },
        )
    }
}

const CLAUDE_PAYLOAD: &str = r#"{"limits":[
        {"kind":"session","group":"session","percent":7,"resets_at":"2026-08-18T04:59:59+00:00"},
        {"kind":"weekly_all","group":"weekly","percent":12,"resets_at":"2026-08-24T13:59:59+00:00"}
    ]}"#;

const CLAUDE_PROFILE: &str =
    r#"{"account":{"uuid":"uuid-1","email":"me@example.com"},"organization":{"uuid":"org-1"}}"#;

fn claude_account_file(id: &str, email: &str) -> String {
    format!(
        r#"{{"oauthAccount":{{"accountUuid":"{id}","emailAddress":"{email}","organizationUuid":"org-1"}}}}"#
    )
}

fn claude_profile(id: &str, email: &str) -> String {
    format!(
        r#"{{"account":{{"uuid":"{id}","email":"{email}"}},"organization":{{"uuid":"org-1"}}}}"#
    )
}

fn account_of(dto: &ProviderLimitsDto) -> LimitsAccountDto {
    dto.account.clone().expect("account")
}

#[test]
fn claude_pipeline_sends_the_oauth_headers_and_maps_the_payload() {
    let home = scratch_dir("limits-claude");
    write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    let lookup = read_claude_credential(&home, Ok(None), NOW_MS);
    let dto = claude_limits(lookup, None, &profile_url, &usage_url).dto;
    let profile_head = profile_request.join().unwrap();
    let usage_head = usage_request.join().unwrap();
    assert!(
        profile_head.contains("Authorization: Bearer kc-token"),
        "{profile_head}"
    );
    assert!(
        profile_head.contains("Cache-Control: no-cache"),
        "{profile_head}"
    );
    assert!(
        usage_head.contains("Authorization: Bearer kc-token"),
        "{usage_head}"
    );
    assert!(
        usage_head.contains("anthropic-beta: oauth-2025-04-20"),
        "{usage_head}"
    );
    assert_eq!(dto.status, LimitsStatus::Ok, "{:?}", dto.message);
    assert_eq!(dto.plan.as_deref(), Some("max"));
    assert_eq!(dto.account, Some(account("uuid-1", "me@example.com")));
    assert!(dto.current_account);
    let ids: Vec<&str> = dto.windows.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["weekly_all", "session"]);
}

#[test]
fn claude_rejects_usage_when_the_authenticated_profile_is_a_different_account() {
    let home = scratch_dir("limits-claude-mismatch");
    write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (url, request) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"uuid-other","email":"other@example.com"},"organization":{"uuid":"org-other"}}"#,
    );
    let lookup = read_claude_credential(&home, Ok(None), NOW_MS);
    let dto = claude_limits(
        lookup,
        Some(ClaudeIdentity {
            account: account("uuid-1", "me@example.com"),
            organization_id: Some("org-1".to_string()),
        }),
        &url,
        &refused_url(),
    )
    .dto;
    request.join().unwrap();

    assert_eq!(dto.status, LimitsStatus::Failed);
    assert_eq!(dto.account, Some(account("uuid-1", "me@example.com")));
    assert!(
        dto.message
            .as_deref()
            .unwrap()
            .contains("different account"),
        "{:?}",
        dto.message
    );
    assert!(dto.windows.is_empty());
}

#[test]
fn claude_rejects_usage_when_the_authenticated_organization_is_different() {
    let home = scratch_dir("limits-claude-org-mismatch");
    write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (profile_url, profile_request) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"uuid-1","email":"me@example.com"},"organization":{"uuid":"org-other"}}"#,
    );
    let lookup = read_claude_credential(&home, Ok(None), NOW_MS);
    let dto = claude_limits(
        lookup,
        Some(ClaudeIdentity {
            account: account("uuid-1", "me@example.com"),
            organization_id: Some("org-1".to_string()),
        }),
        &profile_url,
        &refused_url(),
    )
    .dto;
    profile_request.join().unwrap();

    assert_eq!(dto.status, LimitsStatus::Failed);
    assert_eq!(dto.account, Some(account("uuid-1", "me@example.com")));
    assert!(dto.windows.is_empty());
}

#[test]
fn claude_account_mismatch_evicts_the_memoized_token_pairing() {
    let mut rig = Rig::new("limits-claude-mismatch-memo");
    rig.keychain_json = Some(CLAUDE_CREDENTIALS.replace("kc-token", "token-old"));
    write(
        &rig.home,
        ".claude.json",
        &claude_account_file("uuid-1", "me@example.com"),
    );
    let (profile_url, profile_request) = serve_once(
        "200 OK",
        r#"{"account":{"uuid":"uuid-other","email":"other@example.com"},"organization":{"uuid":"org-1"}}"#,
    );
    let first = rig.read(AgentId::Claude, false, &profile_url, &refused_url());
    assert!(profile_request.join().unwrap().contains("Bearer token-old"));
    assert_eq!(first[0].status, LimitsStatus::Failed);

    rig.keychain_json = Some(CLAUDE_CREDENTIALS.replace("kc-token", "token-new"));
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    let second = rig.read(AgentId::Claude, false, &profile_url, &usage_url);
    let profile_head = profile_request.join().unwrap();
    usage_request.join().unwrap();

    assert!(profile_head.contains("Bearer token-new"), "{profile_head}");
    assert_eq!(rig.probes.get(), 2);
    assert_eq!(
        second[0].status,
        LimitsStatus::Ok,
        "{:?}",
        second[0].message
    );
}

#[test]
fn claude_read_prefers_the_keychain_login_and_uses_the_authenticated_profile_identity() {
    let mut rig = Rig::new("limits-claude");
    rig.keychain_json = Some(CLAUDE_CREDENTIALS.replace("kc-token", "keychain-token"));
    write(&rig.home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    let dtos = rig.read(AgentId::Claude, false, &profile_url, &usage_url);
    assert!(profile_request
        .join()
        .unwrap()
        .contains("Authorization: Bearer keychain-token"));
    usage_request.join().unwrap();
    assert_eq!(dtos[0].status, LimitsStatus::Ok, "{:?}", dtos[0].message);
    assert_eq!(account_of(&dtos[0]).id, "uuid-1");
    assert_eq!(rig.probes.get(), 1);
}

#[test]
fn claude_read_skips_the_network_when_expired_or_signed_out() {
    let rig = Rig::new("limits-claude");
    assert_eq!(
        rig.read(AgentId::Claude, false, &refused_url(), &refused_url())[0].status,
        LimitsStatus::SignedOut
    );
    write(&rig.home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let expired = read_limits_in(
        AgentId::Claude,
        false,
        Sources {
            home: &rig.home,
            memo: &rig.memo,
            keychain: || Ok(None),
            claude_profile_url: &refused_url(),
            claude_url: &refused_url(),
            claude_desktop_history: claude_desktop::history_path_for_home(&rig.home),
            now_ms: 1787022473402 + 1,
        },
    );
    assert_eq!(expired[0].status, LimitsStatus::Unauthenticated);
    assert!(expired[0].message.as_deref().unwrap().contains("`claude`"));
}

#[test]
fn claude_read_memoises_the_keychain_until_forced_and_forgets_it_when_rejected() {
    let mut rig = Rig::new("limits-claude");
    rig.keychain_json = Some(CLAUDE_CREDENTIALS.to_string());
    write(
        &rig.home,
        ".claude.json",
        &claude_account_file("uuid-1", "me@example.com"),
    );

    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    rig.read(AgentId::Claude, false, &profile_url, &usage_url);
    profile_request.join().unwrap();
    usage_request.join().unwrap();
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    rig.read(AgentId::Claude, false, &profile_url, &usage_url);
    profile_request.join().unwrap();
    usage_request.join().unwrap();
    assert_eq!(
        rig.probes.get(),
        1,
        "second non-forced read served from the memo"
    );

    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    rig.read(AgentId::Claude, true, &profile_url, &usage_url);
    profile_request.join().unwrap();
    usage_request.join().unwrap();
    assert_eq!(
        rig.probes.get(),
        2,
        "an explicit refresh re-reads the Keychain"
    );

    // The memoised token is rejected, so the stored login is read again and tried once more;
    // rejected a second time, the login really is the problem.
    let (profile_url, profile_requests) = serve_sequence(&[
        ("401 Unauthorized", &[], "{}"),
        ("401 Unauthorized", &[], "{}"),
    ]);
    let rejected = rig.read(AgentId::Claude, false, &profile_url, &refused_url());
    profile_requests.join().unwrap();
    assert_eq!(rejected[0].status, LimitsStatus::Unauthenticated);
    let message = rejected[0].message.as_deref().unwrap_or_default();
    assert!(
        message.contains("was rejected") && !message.contains("expired"),
        "a login re-read moments ago has not expired; say what happened: {message}"
    );
    assert_eq!(
        rig.probes.get(),
        3,
        "the rejection re-read the Keychain once before blaming the login"
    );
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    rig.read(AgentId::Claude, false, &profile_url, &usage_url);
    profile_request.join().unwrap();
    usage_request.join().unwrap();
    assert_eq!(rig.probes.get(), 4, "a rejected token evicts the memo");
}

#[test]
fn a_rotated_token_is_retried_from_the_keychain_before_blaming_the_login() {
    // Claude Code rotates the access token before its recorded `expiresAt`, which invalidates the
    // one the memo is holding. The rejection says nothing about the login itself, so the read
    // re-reads the Keychain and tries once more rather than telling the user to sign in again.
    let mut rig = Rig::new("limits-claude-rotated");
    rig.keychain_json = Some(CLAUDE_RENEWABLE_CREDENTIALS.replace("kc-token", "old-token"));
    write(
        &rig.home,
        ".claude.json",
        &claude_account_file("uuid-1", "me@example.com"),
    );

    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    rig.read(AgentId::Claude, false, &profile_url, &usage_url);
    profile_request.join().unwrap();
    usage_request.join().unwrap();
    assert_eq!(rig.probes.get(), 1, "the login is memoised for the app run");

    // Claude Code has since written a new token; the memoised one now gets a 401.
    rig.keychain_json = Some(CLAUDE_RENEWABLE_CREDENTIALS.replace("kc-token", "new-token"));
    let (profile_url, profile_requests) = serve_sequence(&[
        ("401 Unauthorized", &[], "{}"),
        ("200 OK", &[], CLAUDE_PROFILE),
    ]);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    let recovered = rig.read(AgentId::Claude, false, &profile_url, &usage_url);
    let profiles = profile_requests.join().unwrap();
    usage_request.join().unwrap();

    assert_eq!(
        recovered[0].status,
        LimitsStatus::Ok,
        "a rotated token is not a dead login: {:?}",
        recovered[0].message
    );
    assert_eq!(
        rig.probes.get(),
        2,
        "the rejection re-read the Keychain once"
    );
    assert_eq!(profiles.len(), 2, "the retry reached the endpoint");
    assert!(
        profiles[0].head.contains("Bearer old-token"),
        "the first attempt used the memoised token"
    );
    assert!(
        profiles[1].head.contains("Bearer new-token"),
        "the retry used the token the Keychain now holds"
    );
}

#[test]
fn a_freshly_read_login_the_endpoint_rejects_is_not_retried() {
    // The retry exists for a memo holding a token Claude Code has rotated past. A login read
    // from the Keychain moments ago and rejected is the login's own problem: asking the endpoint
    // the same question again would only cost a second Keychain probe, which is the prompt the
    // memo exists to avoid.
    let mut rig = Rig::new("limits-claude-fresh-rejected");
    rig.keychain_json = Some(CLAUDE_RENEWABLE_CREDENTIALS.to_string());
    write(
        &rig.home,
        ".claude.json",
        &claude_account_file("uuid-1", "me@example.com"),
    );

    let (profile_url, profile_requests) = serve_sequence(&[("401 Unauthorized", &[], "{}")]);
    let rejected = rig.read(AgentId::Claude, false, &profile_url, &refused_url());
    let profiles = profile_requests.join().unwrap();

    assert_eq!(rejected[0].status, LimitsStatus::Unauthenticated);
    assert_eq!(
        profiles.len(),
        1,
        "the first read of the run was not memoised, so there was no stale token to retry past"
    );
    assert_eq!(
        rig.probes.get(),
        1,
        "and the Keychain is probed once, not twice"
    );
}

#[test]
fn a_re_read_that_finds_no_login_keeps_the_rejection_it_already_has() {
    // The retry exists to try a newer credential. When the store has none to give, the rejection
    // already in hand is the accurate answer: reporting the miss instead would tell a signed-in
    // user to sign in, which is a louder falsehood than the one this retry removes.
    let mut rig = Rig::new("limits-claude-reread-miss");
    rig.keychain_json = Some(CLAUDE_RENEWABLE_CREDENTIALS.to_string());
    write(
        &rig.home,
        ".claude.json",
        &claude_account_file("uuid-1", "me@example.com"),
    );

    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    rig.read(AgentId::Claude, false, &profile_url, &usage_url);
    profile_request.join().unwrap();
    usage_request.join().unwrap();

    // The memoised token is refused, and by then the stored login has gone.
    rig.keychain_json = None;
    let (profile_url, profile_requests) = serve_sequence(&[("401 Unauthorized", &[], "{}")]);
    let rejected = rig.read(AgentId::Claude, false, &profile_url, &refused_url());
    let profiles = profile_requests.join().unwrap();

    assert_eq!(
        profiles.len(),
        1,
        "a re-read with nothing to show gives the endpoint nothing new to answer"
    );
    assert_eq!(rejected[0].status, LimitsStatus::Unauthenticated);
    let message = rejected[0].message.as_deref().unwrap_or_default();
    assert!(
        message.contains("was rejected"),
        "the rejection stands, rather than being replaced by the miss: {message}"
    );
    assert!(
        !message.contains("Sign in with"),
        "the user is signed in; only the re-read failed: {message}"
    );
}

#[test]
fn an_explicit_refresh_that_is_rejected_is_not_retried() {
    // `force` already read the stored login, so the rejection is of the freshest credential there
    // is. Retrying would ask the endpoint the same question over a second Keychain probe.
    let mut rig = Rig::new("limits-claude-forced-rejected");
    rig.keychain_json = Some(CLAUDE_RENEWABLE_CREDENTIALS.to_string());
    write(
        &rig.home,
        ".claude.json",
        &claude_account_file("uuid-1", "me@example.com"),
    );

    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    rig.read(AgentId::Claude, false, &profile_url, &usage_url);
    profile_request.join().unwrap();
    usage_request.join().unwrap();
    assert_eq!(rig.probes.get(), 1);

    let (profile_url, profile_requests) = serve_sequence(&[("401 Unauthorized", &[], "{}")]);
    let rejected = rig.read(AgentId::Claude, true, &profile_url, &refused_url());
    let profiles = profile_requests.join().unwrap();

    assert_eq!(rejected[0].status, LimitsStatus::Unauthenticated);
    assert_eq!(profiles.len(), 1, "a forced read is already fresh");
    assert_eq!(
        rig.probes.get(),
        2,
        "the forced read probed once; the rejection added no second probe"
    );
}

#[test]
fn switching_the_claude_account_never_reuses_the_previous_accounts_token() {
    let mut rig = Rig::new("limits-claude");
    rig.keychain_json = Some(CLAUDE_CREDENTIALS.replace("kc-token", "token-a"));
    write(
        &rig.home,
        ".claude.json",
        &claude_account_file("uuid-a", "a@example.com"),
    );
    let (profile_url, profile_request) =
        serve_once("200 OK", &claude_profile("uuid-a", "a@example.com"));
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    let first = rig.read(AgentId::Claude, false, &profile_url, &usage_url);
    assert!(profile_request.join().unwrap().contains("Bearer token-a"));
    usage_request.join().unwrap();
    assert_eq!(
        account_of(&first[0]).label.as_deref(),
        Some("a@example.com")
    );

    // The user runs `claude` and signs in as B: Claude Code rewrites both stores.
    rig.keychain_json = Some(CLAUDE_CREDENTIALS.replace("kc-token", "token-b"));
    write(
        &rig.home,
        ".claude.json",
        &claude_account_file("uuid-b", "b@example.com"),
    );
    let (profile_url, profile_request) =
        serve_once("200 OK", &claude_profile("uuid-b", "b@example.com"));
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    let second = rig.read(AgentId::Claude, false, &profile_url, &usage_url);
    assert!(
        profile_request.join().unwrap().contains("Bearer token-b"),
        "B's card must be fetched with B's token, not the memoised A token"
    );
    usage_request.join().unwrap();
    assert_eq!(rig.probes.get(), 2);
    let ids: Vec<(String, bool)> = second
        .iter()
        .map(|dto| (account_of(dto).id, dto.current_account))
        .collect();
    assert_eq!(
        ids,
        [("uuid-b".to_string(), true), ("uuid-a".to_string(), false)]
    );
}

#[test]
fn providers_without_a_subscription_are_unsupported() {
    for provider in [AgentId::Cursor, AgentId::Antigravity] {
        let dtos = read_limits(provider, false);
        assert_eq!(dtos.len(), 1);
        assert_eq!(dtos[0].provider, provider);
        assert_eq!(dtos[0].status, LimitsStatus::Unsupported);
        assert!(dtos[0].message.is_some());
    }
}

#[test]
fn dto_serializes_with_the_camel_case_wire_shape_the_ui_expects() {
    let ok = ProviderLimitsDto {
        provider: AgentId::Codex,
        status: LimitsStatus::Ok,
        message: None,
        account: Some(LimitsAccountDto {
            id: "acct-1".to_string(),
            label: Some("me@example.com".to_string()),
        }),
        current_account: true,
        plan: Some("pro".to_string()),
        windows: vec![LimitWindowDto {
            observed_at: "2026-08-17T20:00:00.000Z".to_string(),
            ..window(
                "primary",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                2.5,
                None,
            )
        }],
        credits: Some(LimitsCreditsDto {
            balance: "3".to_string(),
            unlimited: false,
        }),
    };
    assert_eq!(
        serde_json::to_value(&ok).unwrap(),
        json!({
            "provider": "codex",
            "status": "ok",
            "account": {"id": "acct-1", "label": "me@example.com"},
            "currentAccount": true,
            "plan": "pro",
            "windows": [{"id": "primary", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 2.5, "observedAt": "2026-08-17T20:00:00.000Z"}],
            "credits": {"balance": "3", "unlimited": false}
        })
    );
    let signed_out = finish(
        AgentId::Claude,
        LimitsStatus::SignedOut,
        Some("Sign in".to_string()),
        Parsed::default(),
    );
    let value = serde_json::to_value(&signed_out).unwrap();
    assert_eq!(value["status"], "signedOut");
    assert_eq!(value["message"], "Sign in");
    assert_eq!(value["windows"], json!([]));
    assert!(value.get("plan").is_none());
    assert!(value.get("credits").is_none());
    assert!(value.get("account").is_none());
    assert_eq!(value["currentAccount"], true);
}

/// Claude Code's access token lasts 8 hours and the CLI renews it from its refresh token on
/// its next run. An expired access token is therefore not an expired login: the card must ask
/// for a `claude` run, never for a new sign-in, and must still name the signed-in account.
#[test]
fn an_expired_access_token_with_a_live_refresh_token_asks_only_for_a_cli_run() {
    let rig = Rig::new("limits-claude");
    write(
        &rig.home,
        ".claude/.credentials.json",
        CLAUDE_RENEWABLE_CREDENTIALS,
    );
    write(
        &rig.home,
        ".claude.json",
        &claude_account_file("uuid-1", "me@example.com"),
    );
    let dtos = read_limits_in(
        AgentId::Claude,
        false,
        Sources {
            home: &rig.home,
            memo: &rig.memo,
            keychain: || Ok(None),
            claude_profile_url: &refused_url(),
            claude_url: &refused_url(),
            claude_desktop_history: claude_desktop::history_path_for_home(&rig.home),
            now_ms: 1787022473402 + 1,
        },
    );
    assert_eq!(dtos[0].status, LimitsStatus::Unauthenticated);
    let message = dtos[0].message.as_deref().unwrap();
    assert!(message.contains("`claude`"), "{message}");
    assert!(message.contains("send a prompt"), "{message}");
    assert!(!message.contains("any"), "{message}");
    assert!(!message.contains("sign in"), "{message}");
    assert_eq!(
        account_of(&dtos[0]).label.as_deref(),
        Some("me@example.com")
    );
}

/// A login whose refresh token has expired too really does need a new sign-in.
#[test]
fn an_expired_access_token_without_a_usable_refresh_token_asks_for_a_new_sign_in() {
    let rig = Rig::new("limits-claude");
    write(
        &rig.home,
        ".claude/.credentials.json",
        &CLAUDE_RENEWABLE_CREDENTIALS.replace("1787981634215", "1787022473402"),
    );
    let dtos = read_limits_in(
        AgentId::Claude,
        false,
        Sources {
            home: &rig.home,
            memo: &rig.memo,
            keychain: || Ok(None),
            claude_profile_url: &refused_url(),
            claude_url: &refused_url(),
            claude_desktop_history: claude_desktop::history_path_for_home(&rig.home),
            now_ms: 1787022473402 + 1,
        },
    );
    assert_eq!(dtos[0].status, LimitsStatus::Unauthenticated);
    assert!(dtos[0]
        .message
        .as_deref()
        .unwrap()
        .contains("sign in again"));
}

/// Live probe against the real home: Keychain read + one GET per provider (read-only).
/// `cargo test --manifest-path src-tauri/Cargo.toml probe_real_home_limits -- --ignored --nocapture`
#[test]
#[ignore = "real-home network probe; not part of CI"]
fn probe_real_home_limits() {
    for provider in [AgentId::Claude, AgentId::Codex] {
        for dto in read_limits(provider, false) {
            println!(
                    "{:?}: current_account={} account={:?} status={:?} plan={:?} message={:?} credits={:?}",
                    provider,
                    dto.current_account,
                    dto.account,
                    dto.status,
                    dto.plan,
                    dto.message,
                    dto.credits
                );
            for window in &dto.windows {
                println!(
                    "  [{:?}] {} ({}) used={}% resets_at={:?} observed_at={}",
                    window.kind,
                    window.label,
                    window.id,
                    window.used_percent,
                    window.resets_at,
                    window.observed_at
                );
            }
        }
    }
}
