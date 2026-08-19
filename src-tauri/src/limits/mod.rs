//! Subscription rate limits: live per-provider snapshot from the vendors' usage endpoints,
//! authenticated with the CLIs' own stored logins (read-only; never refreshed here).
//!
//! `read_limits` never fails for provider-side reasons; every outcome is a `ProviderLimitsDto`
//! whose `status` + `message` tell the UI what to show. Because each CLI stores one login at a
//! time, successful reads are also remembered per account (numbers only) so accounts the user
//! has switched away from stay visible as of their last read.

mod claude;
mod codex;
mod credentials;
mod http;
mod json;
mod snapshots;

use std::path::Path;

use chrono::{SecondsFormat, Utc};

use crate::dto::{
    AgentId, LimitWindowDto, LimitWindowKind, LimitsAccountDto, LimitsCreditsDto, LimitsStatus,
    ProviderLimitsDto,
};
use crate::paths;
use credentials::{
    read_claude_account, read_claude_credential, read_codex_credential, ClaudeCredential,
    ClaudeLoginMemo, CredentialLookup, KeychainProbe, CLAUDE_LOGIN,
};
use http::{get_json, HttpError};
use snapshots::SnapshotStore;

const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
/// Account id used when the CLI stores no identity; keeps single-account behaviour intact.
const DEFAULT_ACCOUNT: &str = "default";

/// Provider-neutral content of a parsed usage payload; `Default` is the empty snapshot that
/// accompanies every non-`ok` status.
#[derive(Debug, Clone, Default, PartialEq)]
struct Parsed {
    account: Option<LimitsAccountDto>,
    plan: Option<String>,
    windows: Vec<LimitWindowDto>,
    credits: Option<LimitsCreditsDto>,
}

/// Everything `read_limits` needs that tests replace: where the homes/snapshots live, the
/// Keychain probe and its memo, and the endpoints.
struct Sources<'a, P: FnOnce() -> KeychainProbe> {
    home: &'a Path,
    memo: &'a ClaudeLoginMemo,
    keychain: P,
    claude_url: &'a str,
    codex_url: &'a str,
    now_ms: i64,
}

/// Live subscription limits for one provider, followed by remembered snapshots of its other
/// accounts. Blocking: runs a Keychain probe (macOS, Claude) and one HTTPS request; call it off
/// the UI thread. `force` = explicit refresh: re-read the Keychain instead of the in-process memo.
pub fn read_limits(agent: AgentId, force: bool) -> Vec<ProviderLimitsDto> {
    let home = match paths::user_home() {
        Ok(home) => home,
        Err(error) => {
            return vec![finish(
                agent,
                LimitsStatus::Failed,
                Some(format!(
                    "Could not read the stored login: {}",
                    error.message
                )),
                Parsed::default(),
            )]
        }
    };
    read_limits_in(
        agent,
        force,
        Sources {
            home: &home,
            memo: &CLAUDE_LOGIN,
            keychain: credentials::keychain_claude_json,
            claude_url: CLAUDE_USAGE_URL,
            codex_url: CODEX_USAGE_URL,
            now_ms: Utc::now().timestamp_millis(),
        },
    )
}

/// Drop the remembered snapshot of one account (the user's "Forget" on a stale card).
pub fn forget_snapshot(agent: AgentId, account_id: &str) -> Result<(), String> {
    let home = paths::user_home().map_err(|error| error.message)?;
    SnapshotStore::for_home(&home).forget(agent, account_id)
}

fn read_limits_in<P: FnOnce() -> KeychainProbe>(
    agent: AgentId,
    force: bool,
    sources: Sources<'_, P>,
) -> Vec<ProviderLimitsDto> {
    let home = sources.home;
    let current = match agent {
        AgentId::Claude => claude_live(force, sources),
        AgentId::Codex => codex_limits(home, sources.codex_url),
        AgentId::Antigravity | AgentId::Cursor => {
            return vec![finish(
                agent,
                LimitsStatus::Unsupported,
                Some(format!(
                    "{} has no subscription limits to show.",
                    agent.display_name()
                )),
                Parsed::default(),
            )]
        }
    };
    with_memory(&SnapshotStore::for_home(home), current)
}

/// Remember a successful live read (best-effort, like the usage caches), then return it followed
/// by the other remembered accounts, newest first. A live account is never listed twice: when its
/// read did not come back `ok`, its last remembered numbers are folded into that one card.
fn with_memory(store: &SnapshotStore, current: ProviderLimitsDto) -> Vec<ProviderLimitsDto> {
    let _ = store.save(&current);
    let live_account = current.account.as_ref().map(|account| account.id.clone());
    let mut remembered = store.load(current.provider);
    let mut live = current;
    if live.status != LimitsStatus::Ok {
        if let Some(id) = live_account.as_deref() {
            if let Some(at) = remembered
                .iter()
                .position(|snapshot| snapshot.account.as_ref().is_some_and(|a| a.id == id))
            {
                live = keep_numbers(live, remembered.remove(at));
            }
        }
    }
    let others = remembered.into_iter().filter(|snapshot| {
        snapshot.account.as_ref().map(|account| &account.id) != live_account.as_ref()
    });
    std::iter::once(live).chain(others).collect()
}

/// A live read that failed for an account we already have numbers for keeps those numbers, dated
/// by the read they came from, under the live status and message. One card per account: never a
/// blank error card stacked on top of the same account's last good read.
fn keep_numbers(live: ProviderLimitsDto, remembered: ProviderLimitsDto) -> ProviderLimitsDto {
    ProviderLimitsDto {
        plan: remembered.plan,
        windows: remembered.windows,
        credits: remembered.credits,
        fetched_at: remembered.fetched_at,
        ..live
    }
}

/// Claude: which account the CLI is signed into (`~/.claude.json`) decides whether the memoised
/// login may be reused; otherwise the Keychain (or the credentials file) is read. A rejected token
/// evicts the memo so the next read goes back to the Keychain.
fn claude_live<P: FnOnce() -> KeychainProbe>(
    force: bool,
    sources: Sources<'_, P>,
) -> ProviderLimitsDto {
    let Sources {
        home,
        memo,
        keychain,
        claude_url,
        now_ms,
        ..
    } = sources;
    let account = read_claude_account(home).unwrap_or_else(default_account);
    let lookup = memo.lookup(force, &account.id, now_ms, || {
        read_claude_credential(home, keychain(), now_ms)
    });
    let dto = claude_limits(lookup, account, claude_url);
    if dto.status == LimitsStatus::Unauthenticated {
        memo.clear();
    }
    dto
}

/// Claude: stored OAuth login → `GET {url}` with the OAuth beta header → normalized windows.
/// The plan label comes from the login itself (`subscriptionType`).
fn claude_limits(
    lookup: CredentialLookup<ClaudeCredential>,
    account: LimitsAccountDto,
    url: &str,
) -> ProviderLimitsDto {
    resolve(
        AgentId::Claude,
        Some(account.clone()),
        lookup,
        move |credential| {
            let bearer = format!("Bearer {}", credential.token);
            let payload = get_json(
                url,
                &[
                    ("Authorization", &bearer),
                    ("anthropic-beta", "oauth-2025-04-20"),
                ],
            )?;
            Ok(Parsed {
                account: Some(account),
                plan: credential.subscription_type.clone(),
                windows: claude::parse_claude(&payload),
                credits: None,
            })
        },
    )
}

/// Codex: `auth.json` ChatGPT login → `GET {url}` with the account header → plan, windows, credits.
/// The account id is the login's `account_id`; its label is the payload's email.
fn codex_limits(home: &Path, url: &str) -> ProviderLimitsDto {
    resolve(
        AgentId::Codex,
        None,
        read_codex_credential(home),
        |credential| {
            let bearer = format!("Bearer {}", credential.token);
            let mut headers: Vec<(&str, &str)> = vec![("Authorization", &bearer)];
            if let Some(account) = credential.account_id.as_deref() {
                headers.push(("ChatGPT-Account-Id", account));
            }
            let payload = get_json(url, &headers)?;
            let mut parsed = codex::parse_codex(&payload);
            parsed.account = Some(LimitsAccountDto {
                id: credential
                    .account_id
                    .clone()
                    .unwrap_or_else(|| DEFAULT_ACCOUNT.to_string()),
                label: codex::account_email(&payload),
            });
            Ok(parsed)
        },
    )
}

fn default_account() -> LimitsAccountDto {
    LimitsAccountDto {
        id: DEFAULT_ACCOUNT.to_string(),
        label: None,
    }
}

/// The provider-neutral pipeline: login state → `load` (fetch + parse) → status. Does no I/O of
/// its own beyond `load`, so it is unit-tested with closures.
fn resolve<T>(
    provider: AgentId,
    account: Option<LimitsAccountDto>,
    lookup: CredentialLookup<T>,
    load: impl FnOnce(&T) -> Result<Parsed, HttpError>,
) -> ProviderLimitsDto {
    let cli = provider.binary_name();
    // Every non-`ok` outcome still names the account it is about, so the UI can put the status on
    // that account's card instead of an anonymous card beside it.
    let named = || Parsed {
        account: account.clone(),
        ..Parsed::default()
    };
    let credential = match lookup {
        CredentialLookup::Found(credential) => credential,
        CredentialLookup::Missing => {
            return finish(
                provider,
                LimitsStatus::SignedOut,
                Some(format!("Sign in with `{cli}` to see subscription limits.")),
                named(),
            )
        }
        CredentialLookup::Expired { renewable } => {
            return finish(
                provider,
                LimitsStatus::Unauthenticated,
                Some(token_expired(cli, renewable)),
                named(),
            )
        }
        CredentialLookup::Unsupported(why) => {
            return finish(provider, LimitsStatus::Unsupported, Some(why), named())
        }
        CredentialLookup::Unreadable(why) => {
            return finish(
                provider,
                LimitsStatus::Failed,
                Some(format!("Could not read the stored login: {why}")),
                named(),
            )
        }
    };
    match load(&credential) {
        Ok(mut parsed) => {
            parsed.windows.sort_by_key(|window| kind_rank(window.kind));
            finish(provider, LimitsStatus::Ok, None, parsed)
        }
        Err(HttpError::Unauthorized) => finish(
            provider,
            LimitsStatus::Unauthenticated,
            Some(relogin(cli)),
            named(),
        ),
        Err(error) => finish(
            provider,
            LimitsStatus::Failed,
            Some(format!(
                "Could not reach the {} usage service ({error}).",
                provider.display_name()
            )),
            named(),
        ),
    }
}

fn finish(
    provider: AgentId,
    status: LimitsStatus,
    message: Option<String>,
    parsed: Parsed,
) -> ProviderLimitsDto {
    ProviderLimitsDto {
        provider,
        status,
        message,
        account: parsed.account,
        live: true,
        plan: parsed.plan,
        windows: parsed.windows,
        credits: parsed.credits,
        fetched_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    }
}

fn relogin(cli: &str) -> String {
    format!("Login expired — run `{cli}` and sign in again to refresh subscription limits.")
}

/// The stored access token has passed its own expiry. Such a token lasts hours and the CLI mints a
/// new one from its refresh token the next time it runs, so only a login that can no longer renew
/// itself needs a new sign-in; asking for one otherwise sends the user through a pointless login.
fn token_expired(cli: &str, renewable: bool) -> String {
    if renewable {
        format!("Access token expired — run any `{cli}` command to renew it, then refresh here.")
    } else {
        relogin(cli)
    }
}

fn kind_rank(kind: LimitWindowKind) -> u8 {
    match kind {
        LimitWindowKind::Weekly => 0,
        LimitWindowKind::Session => 1,
        LimitWindowKind::Model => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::scratch_dir;
    use http::{refused_url, serve_once};
    use json::window;
    use serde_json::json;
    use std::cell::Cell;
    use std::fs;

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

    fn ok_snapshot(provider: AgentId, id: &str, label: &str, used: f64) -> ProviderLimitsDto {
        let mut dto = finish(
            provider,
            LimitsStatus::Ok,
            None,
            Parsed {
                account: Some(account(id, label)),
                plan: Some("pro".to_string()),
                windows: vec![window(
                    "primary",
                    "Weekly · all models",
                    LimitWindowKind::Weekly,
                    used,
                    None,
                )],
                credits: None,
            },
        );
        dto.fetched_at = format!("2026-08-17T{:02}:00:00.000Z", used as u32 % 24);
        dto
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
    const CODEX_AUTH: &str =
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"codex-token","account_id":"acct-1"}}"#;
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
    fn expired_unsupported_and_unreadable_logins_map_to_their_statuses_without_loading() {
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

        let unsupported = resolve(
            AgentId::Codex,
            None,
            CredentialLookup::Unsupported("API key".to_string()),
            load,
        );
        assert_eq!(unsupported.status, LimitsStatus::Unsupported);
        assert_eq!(unsupported.message.as_deref(), Some("API key"));

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
        chrono::DateTime::parse_from_rfc3339(&dto.fetched_at).expect("fetched_at is RFC 3339");
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
            claude_url: &str,
            codex_url: &str,
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
                    claude_url,
                    codex_url,
                    now_ms: NOW_MS,
                },
            )
        }
    }

    const CLAUDE_PAYLOAD: &str = r#"{"limits":[
        {"kind":"session","group":"session","percent":7,"resets_at":"2026-08-18T04:59:59+00:00"},
        {"kind":"weekly_all","group":"weekly","percent":12,"resets_at":"2026-08-24T13:59:59+00:00"}
    ]}"#;

    fn claude_account_file(id: &str, email: &str) -> String {
        format!(r#"{{"oauthAccount":{{"accountUuid":"{id}","emailAddress":"{email}"}}}}"#)
    }

    fn account_of(dto: &ProviderLimitsDto) -> LimitsAccountDto {
        dto.account.clone().expect("account")
    }

    #[test]
    fn claude_pipeline_sends_the_oauth_headers_and_maps_the_payload() {
        let home = scratch_dir("limits-claude");
        write(&home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
        let (url, request) = serve_once("200 OK", CLAUDE_PAYLOAD);
        let lookup = read_claude_credential(&home, Ok(None), NOW_MS);
        let dto = claude_limits(lookup, account("uuid-1", "me@example.com"), &url);
        let head = request.join().unwrap();
        assert!(head.contains("Authorization: Bearer kc-token"), "{head}");
        assert!(head.contains("anthropic-beta: oauth-2025-04-20"), "{head}");
        assert_eq!(dto.status, LimitsStatus::Ok, "{:?}", dto.message);
        assert_eq!(dto.plan.as_deref(), Some("max"));
        assert_eq!(dto.account, Some(account("uuid-1", "me@example.com")));
        assert!(dto.live);
        let ids: Vec<&str> = dto.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["weekly_all", "session"]);
    }

    #[test]
    fn claude_read_prefers_the_keychain_login_and_defaults_the_account_without_claude_json() {
        let mut rig = Rig::new("limits-claude");
        rig.keychain_json = Some(CLAUDE_CREDENTIALS.replace("kc-token", "keychain-token"));
        write(&rig.home, ".claude/.credentials.json", CLAUDE_CREDENTIALS);
        let (url, request) = serve_once("200 OK", CLAUDE_PAYLOAD);
        let dtos = rig.read(AgentId::Claude, false, &url, &refused_url());
        assert!(request
            .join()
            .unwrap()
            .contains("Authorization: Bearer keychain-token"));
        assert_eq!(dtos[0].status, LimitsStatus::Ok, "{:?}", dtos[0].message);
        assert_eq!(account_of(&dtos[0]).id, "default");
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
                claude_url: &refused_url(),
                codex_url: &refused_url(),
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

        let (url, request) = serve_once("200 OK", CLAUDE_PAYLOAD);
        rig.read(AgentId::Claude, false, &url, &refused_url());
        request.join().unwrap();
        let (url, request) = serve_once("200 OK", CLAUDE_PAYLOAD);
        rig.read(AgentId::Claude, false, &url, &refused_url());
        request.join().unwrap();
        assert_eq!(
            rig.probes.get(),
            1,
            "second non-forced read served from the memo"
        );

        let (url, request) = serve_once("200 OK", CLAUDE_PAYLOAD);
        rig.read(AgentId::Claude, true, &url, &refused_url());
        request.join().unwrap();
        assert_eq!(
            rig.probes.get(),
            2,
            "an explicit refresh re-reads the Keychain"
        );

        let (url, request) = serve_once("401 Unauthorized", "{}");
        let rejected = rig.read(AgentId::Claude, false, &url, &refused_url());
        request.join().unwrap();
        assert_eq!(rejected[0].status, LimitsStatus::Unauthenticated);
        assert_eq!(rig.probes.get(), 2);
        let (url, request) = serve_once("200 OK", CLAUDE_PAYLOAD);
        rig.read(AgentId::Claude, false, &url, &refused_url());
        request.join().unwrap();
        assert_eq!(rig.probes.get(), 3, "a rejected token evicts the memo");
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
        let (url, request) = serve_once("200 OK", CLAUDE_PAYLOAD);
        let first = rig.read(AgentId::Claude, false, &url, &refused_url());
        assert!(request.join().unwrap().contains("Bearer token-a"));
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
        let (url, request) = serve_once("200 OK", CLAUDE_PAYLOAD);
        let second = rig.read(AgentId::Claude, false, &url, &refused_url());
        assert!(
            request.join().unwrap().contains("Bearer token-b"),
            "B's card must be fetched with B's token, not the memoised A token"
        );
        assert_eq!(rig.probes.get(), 2);
        let ids: Vec<(String, bool)> = second
            .iter()
            .map(|dto| (account_of(dto).id, dto.live))
            .collect();
        assert_eq!(
            ids,
            [("uuid-b".to_string(), true), ("uuid-a".to_string(), false)]
        );
    }

    #[test]
    fn codex_read_remembers_the_account_and_lists_it_live_first() {
        let rig = Rig::new("limits-codex");
        let store = SnapshotStore::for_home(&rig.home);
        store
            .save(&ok_snapshot(AgentId::Codex, "acct-old", "old@x", 5.0))
            .unwrap();
        write(&rig.home, ".codex/auth.json", CODEX_AUTH);
        let (url, request) = serve_once(
            "200 OK",
            r#"{"email":"me@example.com","plan_type":"pro","rate_limit":{"primary_window":{"used_percent":2,"limit_window_seconds":604800}}}"#,
        );
        let dtos = rig.read(AgentId::Codex, false, &refused_url(), &url);
        request.join().unwrap();
        let ids: Vec<(String, bool)> = dtos
            .iter()
            .map(|dto| (account_of(dto).id, dto.live))
            .collect();
        assert_eq!(
            ids,
            [
                ("acct-1".to_string(), true),
                ("acct-old".to_string(), false)
            ]
        );
        assert_eq!(store.load(AgentId::Codex).len(), 2);
        assert_eq!(rig.probes.get(), 0, "Codex never touches the Keychain");

        store.forget(AgentId::Codex, "acct-old").unwrap();
        let (url, request) = serve_once("200 OK", r#"{"plan_type":"pro"}"#);
        let after = rig.read(AgentId::Codex, false, &refused_url(), &url);
        request.join().unwrap();
        assert_eq!(after.len(), 1);
    }

    #[test]
    fn codex_pipeline_sends_the_account_header_and_maps_the_payload() {
        let home = scratch_dir("limits-codex");
        write(&home, ".codex/auth.json", CODEX_AUTH);
        let (url, request) = serve_once(
            "200 OK",
            r#"{"email":"me@example.com","plan_type":"pro","rate_limit":{"primary_window":{"used_percent":2,"limit_window_seconds":604800,"reset_at":1787614473}},
                "additional_rate_limits":[{"limit_name":"Spark","metered_feature":"codex_bengalfox","rate_limit":{"primary_window":{"used_percent":0,"limit_window_seconds":604800}}}],
                "credits":{"has_credits":true,"unlimited":false,"balance":"3"}}"#,
        );
        let dto = codex_limits(&home, &url);
        let head = request.join().unwrap();
        assert!(head.contains("Authorization: Bearer codex-token"), "{head}");
        assert!(head.contains("ChatGPT-Account-Id: acct-1"), "{head}");
        assert_eq!(dto.status, LimitsStatus::Ok, "{:?}", dto.message);
        assert_eq!(dto.plan.as_deref(), Some("pro"));
        assert_eq!(dto.account, Some(account("acct-1", "me@example.com")));
        let ids: Vec<&str> = dto.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["primary", "extra:codex_bengalfox"]);
        assert_eq!(
            dto.credits,
            Some(LimitsCreditsDto {
                balance: "3".to_string(),
                unlimited: false
            })
        );
    }

    #[test]
    fn codex_pipeline_maps_a_rejected_token_to_unauthenticated() {
        let home = scratch_dir("limits-codex");
        write(&home, ".codex/auth.json", CODEX_AUTH);
        let (url, request) = serve_once("401 Unauthorized", r#"{"detail":"expired"}"#);
        let dto = codex_limits(&home, &url);
        request.join().unwrap();
        assert_eq!(dto.status, LimitsStatus::Unauthenticated);
        assert!(dto.message.as_deref().unwrap().contains("`codex`"));
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
    fn a_live_read_is_remembered_and_listed_once_ahead_of_other_accounts() {
        let home = scratch_dir("limits-memory");
        let store = SnapshotStore::for_home(&home);
        store
            .save(&ok_snapshot(AgentId::Codex, "acct-a", "a@x", 5.0))
            .unwrap();
        store
            .save(&ok_snapshot(AgentId::Codex, "acct-b", "b@x", 9.0))
            .unwrap();

        let current = ok_snapshot(AgentId::Codex, "acct-a", "a@x", 50.0);
        let listed = with_memory(&store, current.clone());
        let summary: Vec<(&str, bool, f64)> = listed
            .iter()
            .map(|dto| {
                (
                    dto.account.as_ref().unwrap().id.as_str(),
                    dto.live,
                    dto.windows[0].used_percent,
                )
            })
            .collect();
        assert_eq!(summary, [("acct-a", true, 50.0), ("acct-b", false, 9.0)]);
        // The live read replaced acct-a's remembered numbers.
        let remembered_a = store
            .load(AgentId::Codex)
            .into_iter()
            .find(|dto| dto.account.as_ref().unwrap().id == "acct-a")
            .unwrap();
        assert_eq!(remembered_a.windows[0].used_percent, 50.0);
    }

    #[test]
    fn a_failed_live_read_hides_no_remembered_account_and_remembers_nothing() {
        let home = scratch_dir("limits-memory");
        let store = SnapshotStore::for_home(&home);
        store
            .save(&ok_snapshot(AgentId::Claude, "uuid-a", "a@x", 5.0))
            .unwrap();
        let signed_out = finish(
            AgentId::Claude,
            LimitsStatus::SignedOut,
            Some("Sign in".to_string()),
            Parsed::default(),
        );
        let listed = with_memory(&store, signed_out);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].status, LimitsStatus::SignedOut);
        assert!(listed[0].live);
        assert_eq!(listed[1].account.as_ref().unwrap().id, "uuid-a");
        assert!(!listed[1].live);
        assert_eq!(store.load(AgentId::Claude).len(), 1);
    }

    #[test]
    fn a_live_read_without_an_account_is_not_remembered() {
        let home = scratch_dir("limits-memory");
        let store = SnapshotStore::for_home(&home);
        let anonymous = finish(
            AgentId::Codex,
            LimitsStatus::Ok,
            None,
            parsed(vec![window(
                "w",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                1.0,
                None,
            )]),
        );
        let listed = with_memory(&store, anonymous);
        assert_eq!(listed.len(), 1);
        assert!(store.load(AgentId::Codex).is_empty());
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
            live: true,
            plan: Some("pro".to_string()),
            windows: vec![window(
                "primary",
                "Weekly · all models",
                LimitWindowKind::Weekly,
                2.5,
                None,
            )],
            credits: Some(LimitsCreditsDto {
                balance: "3".to_string(),
                unlimited: false,
            }),
            fetched_at: "2026-08-17T20:00:00.000Z".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&ok).unwrap(),
            json!({
                "provider": "codex",
                "status": "ok",
                "account": {"id": "acct-1", "label": "me@example.com"},
                "live": true,
                "plan": "pro",
                "windows": [{"id": "primary", "label": "Weekly · all models", "kind": "weekly", "usedPercent": 2.5}],
                "credits": {"balance": "3", "unlimited": false},
                "fetchedAt": "2026-08-17T20:00:00.000Z"
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
        assert_eq!(value["live"], true);
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
                claude_url: &refused_url(),
                codex_url: &refused_url(),
                now_ms: 1787022473402 + 1,
            },
        );
        assert_eq!(dtos[0].status, LimitsStatus::Unauthenticated);
        let message = dtos[0].message.as_deref().unwrap();
        assert!(message.contains("`claude`"), "{message}");
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
                claude_url: &refused_url(),
                codex_url: &refused_url(),
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

    #[test]
    fn a_failed_live_read_keeps_the_signed_in_accounts_last_numbers_in_one_card() {
        let home = scratch_dir("limits-memory");
        let store = SnapshotStore::for_home(&home);
        store
            .save(&ok_snapshot(AgentId::Claude, "uuid-a", "a@x", 39.0))
            .unwrap();
        store
            .save(&ok_snapshot(AgentId::Claude, "uuid-b", "b@x", 5.0))
            .unwrap();
        let stalled = finish(
            AgentId::Claude,
            LimitsStatus::Unauthenticated,
            Some("Access token expired".to_string()),
            Parsed {
                account: Some(account("uuid-a", "a@x")),
                ..Parsed::default()
            },
        );
        let listed = with_memory(&store, stalled);

        assert_eq!(listed.len(), 2, "no blank card above the account's numbers");
        assert_eq!(listed[0].status, LimitsStatus::Unauthenticated);
        assert_eq!(listed[0].message.as_deref(), Some("Access token expired"));
        assert!(listed[0].live, "it is still the signed-in account");
        assert_eq!(listed[0].windows[0].used_percent, 39.0);
        assert_eq!(listed[0].plan.as_deref(), Some("pro"));
        assert_eq!(
            listed[0].fetched_at, "2026-08-17T15:00:00.000Z",
            "the card is dated by the numbers it shows"
        );
        assert_eq!(listed[1].account.as_ref().unwrap().id, "uuid-b");
        assert!(!listed[1].live);
    }

    /// Live probe against the real home: Keychain read + one GET per provider (read-only).
    /// `cargo test --manifest-path src-tauri/Cargo.toml probe_real_home_limits -- --ignored --nocapture`
    #[test]
    #[ignore = "real-home network probe; not part of CI"]
    fn probe_real_home_limits() {
        for provider in [AgentId::Claude, AgentId::Codex] {
            for dto in read_limits(provider, true) {
                println!(
                    "{:?}: live={} account={:?} status={:?} plan={:?} message={:?} credits={:?} fetched_at={}",
                    provider, dto.live, dto.account, dto.status, dto.plan, dto.message, dto.credits, dto.fetched_at
                );
                for window in &dto.windows {
                    println!(
                        "  [{:?}] {} ({}) used={}% resets_at={:?}",
                        window.kind, window.label, window.id, window.used_percent, window.resets_at
                    );
                }
            }
        }
    }
}
