//! Reading the Claude login for a limits read, from the outside: the Keychain probe, the login
//! memoised for the app run, and the one retry a token Claude Code rotated earns.

use super::*;

#[test]
fn claude_account_mismatch_evicts_the_memoized_token_pairing() {
    let mut rig = Rig::new("limits-claude-mismatch-memo");
    rig.keychain = Ok(Some(CLAUDE_CREDENTIALS.replace("kc-token", "token-old")));
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

    rig.keychain = Ok(Some(CLAUDE_CREDENTIALS.replace("kc-token", "token-new")));
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
    rig.keychain = Ok(Some(
        CLAUDE_CREDENTIALS.replace("kc-token", "keychain-token"),
    ));
    write(&rig.home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
    let (profile_url, profile_request) = serve_once("200 OK", CLAUDE_PROFILE);
    let (usage_url, usage_request) = serve_once("200 OK", CLAUDE_PAYLOAD);
    let dtos = rig.read(AgentId::Claude, false, &profile_url, &usage_url);
    assert_eq!(
        head_header(&profile_request.join().unwrap(), "authorization"),
        Some("Bearer keychain-token")
    );
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
    let refused = refused_url();
    let expired = read_limits_in(
        AgentId::Claude,
        false,
        Sources {
            home: &rig.home,
            memo: &rig.memo,
            keychain: || Ok(None),
            claude: refused_endpoints(&refused),
            now_ms: 1787022473402 + 1,
        },
    );
    assert_eq!(expired[0].status, LimitsStatus::Unauthenticated);
    assert!(expired[0].message.as_deref().unwrap().contains("`claude`"));
}

#[test]
fn claude_read_memoises_the_keychain_until_forced_and_forgets_it_when_rejected() {
    let mut rig = Rig::new("limits-claude");
    rig.keychain = Ok(Some(CLAUDE_CREDENTIALS.to_string()));
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
    rig.keychain = Ok(Some(
        CLAUDE_RENEWABLE_CREDENTIALS.replace("kc-token", "old-token"),
    ));
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
    rig.keychain = Ok(Some(
        CLAUDE_RENEWABLE_CREDENTIALS.replace("kc-token", "new-token"),
    ));
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
    rig.keychain = Ok(Some(CLAUDE_RENEWABLE_CREDENTIALS.to_string()));
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
    rig.keychain = Ok(Some(CLAUDE_RENEWABLE_CREDENTIALS.to_string()));
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
    rig.keychain = Ok(None);
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
fn a_re_read_the_keychain_refuses_keeps_the_rejection_it_already_has() {
    // The prompt this retry raises can go unanswered — it is raised by a background poll, at a
    // user who chose "Allow" rather than "Always Allow" — and the read then fails on its deadline.
    // That failure describes the probe, not the login, so it must not replace the rejection.
    let mut rig = Rig::new("limits-claude-reread-refused");
    rig.keychain = Ok(Some(CLAUDE_RENEWABLE_CREDENTIALS.to_string()));
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

    rig.keychain = Err("Keychain prompt was not answered in time".to_string());
    let (profile_url, profile_requests) = serve_sequence(&[("401 Unauthorized", &[], "{}")]);
    let rejected = rig.read(AgentId::Claude, false, &profile_url, &refused_url());
    let profiles = profile_requests.join().unwrap();

    assert_eq!(profiles.len(), 1, "there was no newer login to try");
    assert_eq!(rejected[0].status, LimitsStatus::Unauthenticated);
    let message = rejected[0].message.as_deref().unwrap_or_default();
    assert!(
        message.contains("was rejected"),
        "the rejection stands: {message}"
    );
    assert!(
        !message.contains("Could not read the stored login"),
        "the probe failed, not the login; the user is told what the endpoint actually said: \
         {message}"
    );
}

#[test]
fn an_explicit_refresh_that_is_rejected_is_not_retried() {
    // `force` already read the stored login, so the rejection is of the freshest credential there
    // is. Retrying would ask the endpoint the same question over a second Keychain probe.
    let mut rig = Rig::new("limits-claude-forced-rejected");
    rig.keychain = Ok(Some(CLAUDE_RENEWABLE_CREDENTIALS.to_string()));
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
    rig.keychain = Ok(Some(CLAUDE_CREDENTIALS.replace("kc-token", "token-a")));
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
    rig.keychain = Ok(Some(CLAUDE_CREDENTIALS.replace("kc-token", "token-b")));
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
        [
            (
                "profile:1ec440b85711ca5d735ff58bd7a386f6575f2ba5b8d13001274cee6ff445b5b4"
                    .to_string(),
                true
            ),
            (
                "profile:7ca98bb7137e6e534d127d9c8c7f969463c49412dacc66652f233c1a6ad48309"
                    .to_string(),
                false
            )
        ]
    );
}
