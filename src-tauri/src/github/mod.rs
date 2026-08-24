//! The GitHub screen's pull requests: authored, review-requested, and assigned, each with the
//! head commit's CI rollup. Auth is borrowed from the `gh` CLI (`gh auth token`); on-n-off never
//! stores a GitHub token and never writes to GitHub. The only on-n-off writes are the last
//! successful read under `~/.on-n-off/github/`, so the screen has something to show at launch.

mod auth;
#[cfg(test)]
mod fixtures;
mod parse;
mod query;
mod snapshot;

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, SecondsFormat, Utc};

use crate::cli::AgentCli;
use crate::dto::{GithubPrListDto, GithubPrsDto, GithubStatus};
use crate::http::{post_json, HttpError};
use crate::paths;
use crate::settings::{self, AppSettings};
use auth::{read_token_with, GhToken, GhTokenMemo, TokenError, GH_TOKEN, TOKEN_DEADLINE};
use query::{request_body, GRAPHQL_URL};

/// Below this many remaining points the reader stops polling until GitHub's window resets, so a
/// runaway elsewhere cannot make this screen the one that exhausts the budget.
const LOW_BUDGET_POINTS: u64 = 50;
/// The in-memory result is served for the poll interval minus this margin, so a screen refetch
/// that lands just before the monitor's poll (or vice versa) still shares one request.
const CACHE_MARGIN_SECS: i64 = 5;
/// How long to pause when GitHub says "rate limited" without saying until when.
const RATE_LIMIT_FALLBACK_SECS: i64 = 60;

static READ_LOCK: Mutex<()> = Mutex::new(());
static READ_MEMO: ReadMemo = ReadMemo::new();

/// Process-wide reader state: the last result and when it was read, plus the instant until which
/// polling is paused for rate-limit reasons.
pub struct ReadMemo(Mutex<ReadMemoState>);

#[derive(Default)]
struct ReadMemoState {
    cached: Option<(i64, GithubPrsDto)>,
    paused_until: Option<i64>,
}

impl ReadMemo {
    pub const fn new() -> Self {
        Self(Mutex::new(ReadMemoState {
            cached: None,
            paused_until: None,
        }))
    }

    fn state(&self) -> MutexGuard<'_, ReadMemoState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Everything `read_prs_in` reaches for, so tests can substitute a scratch home, a stub `gh`, a
/// loopback endpoint, fresh memos, and a fixed clock.
struct Sources<'a> {
    home: &'a Path,
    settings: &'a AppSettings,
    token_memo: &'a GhTokenMemo,
    read_memo: &'a ReadMemo,
    cli: &'a AgentCli,
    url: &'a str,
    now_secs: i64,
}

/// The GitHub screen's pull requests. Never fails: every problem is a `status` + `hint`, backed
/// by the last snapshot when there is one. `force` skips the in-memory result but never re-runs
/// `gh auth token`; the token is only re-read after GitHub rejects it.
pub fn read_prs(force: bool) -> GithubPrsDto {
    let _guard = READ_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let settings = settings::load_settings();
    let home = match paths::user_home() {
        Ok(home) => home,
        Err(error) => {
            return empty(
                GithubStatus::Network,
                format!("Could not find the home directory: {}", error.message),
                &settings,
            )
        }
    };
    let cli = AgentCli::new("gh");
    read_prs_in(
        &Sources {
            home: &home,
            settings: &settings,
            token_memo: &GH_TOKEN,
            read_memo: &READ_MEMO,
            cli: &cli,
            url: GRAPHQL_URL,
            now_secs: Utc::now().timestamp(),
        },
        force,
    )
}

fn read_prs_in(sources: &Sources<'_>, force: bool) -> GithubPrsDto {
    let window = i64::from(sources.settings.github_poll_seconds) - CACHE_MARGIN_SECS;
    let snapshot_path = paths::github_prs_path_for(sources.home);
    {
        let state = sources.read_memo.state();
        if !force {
            if let Some((read_at, cached)) = &state.cached {
                if sources.now_secs - read_at < window {
                    return cached.clone();
                }
            }
        }
        if let Some(until) = state.paused_until {
            if sources.now_secs < until {
                let paused = empty(
                    GithubStatus::RateLimited,
                    rate_limit_hint(until, sources.now_secs),
                    sources.settings,
                );
                return with_snapshot(paused, &snapshot_path);
            }
        }
    }
    match fetch(sources) {
        Ok(dto) => {
            if let Err(error) = snapshot::save(&snapshot_path, &dto) {
                eprintln!("could not remember the GitHub read: {error}");
            }
            sources.read_memo.state().cached = Some((sources.now_secs, dto.clone()));
            dto
        }
        Err((status, hint)) => with_snapshot(empty(status, hint, sources.settings), &snapshot_path),
    }
}

fn fetch(sources: &Sources<'_>) -> Result<GithubPrsDto, (GithubStatus, String)> {
    let read_token = || read_token_with(sources.cli, TOKEN_DEADLINE);
    let token = sources
        .token_memo
        .lookup(read_token)
        .map_err(token_failure)?;
    let body = request_body(&sources.settings.github_scopes);
    let reply = match post_json(sources.url, token.as_str(), &body) {
        Err(HttpError::Unauthorized) => {
            // The memoised token may be one `gh` has since rotated: ask `gh` again, once.
            sources.token_memo.clear();
            let token: GhToken = sources
                .token_memo
                .lookup(read_token)
                .map_err(token_failure)?;
            post_json(sources.url, token.as_str(), &body).map_err(|error| {
                if error == HttpError::Unauthorized {
                    sources.token_memo.clear();
                }
                http_failure(sources, error)
            })?
        }
        Err(error) => return Err(http_failure(sources, error)),
        Ok(reply) => reply,
    };
    let parsed = parse::parse(&reply).map_err(|why| {
        (
            GithubStatus::Network,
            format!("GitHub sent an unreadable reply ({why})."),
        )
    })?;
    if let Some(limit) = &parsed.rate_limit {
        if limit.remaining < LOW_BUDGET_POINTS {
            let until = DateTime::parse_from_rfc3339(&limit.reset_at)
                .map_or(sources.now_secs + RATE_LIMIT_FALLBACK_SECS, |at| {
                    at.timestamp()
                });
            sources.read_memo.state().paused_until = Some(until);
        }
    }
    Ok(GithubPrsDto {
        status: GithubStatus::Ok,
        hint: None,
        stale: false,
        viewer: parsed.viewer,
        fetched_at: rfc3339(sources.now_secs),
        scope: sources.settings.github_scopes.clone(),
        mine: parsed.mine,
        review_requested: parsed.review_requested,
        assigned: parsed.assigned,
        rate_limit: parsed.rate_limit,
        warnings: parsed.warnings,
    })
}

fn token_failure(error: TokenError) -> (GithubStatus, String) {
    match error {
        TokenError::GhMissing => (
            GithubStatus::GhMissing,
            "Install the GitHub CLI (`gh`), sign in with `gh auth login`, then refresh.".into(),
        ),
        TokenError::NotLoggedIn => (
            GithubStatus::GhNotLoggedIn,
            "Sign in with `gh auth login` in a terminal, then refresh.".into(),
        ),
        TokenError::TimedOut => (
            GithubStatus::GhNotLoggedIn,
            "`gh auth token` did not answer in time — check `gh auth status` in a terminal, then refresh."
                .into(),
        ),
        TokenError::Spawn(why) => (
            GithubStatus::GhMissing,
            format!("Could not start `gh` ({why})."),
        ),
    }
}

fn http_failure(sources: &Sources<'_>, error: HttpError) -> (GithubStatus, String) {
    match error {
        HttpError::Unauthorized => (
            GithubStatus::TokenRejected,
            "GitHub rejected the `gh` login — run `gh auth login` again, then refresh.".into(),
        ),
        HttpError::RateLimited { reset_epoch_secs } => {
            let until = reset_epoch_secs
                .filter(|at| *at > sources.now_secs)
                .unwrap_or(sources.now_secs + RATE_LIMIT_FALLBACK_SECS);
            sources.read_memo.state().paused_until = Some(until);
            (
                GithubStatus::RateLimited,
                rate_limit_hint(until, sources.now_secs),
            )
        }
        other => (
            GithubStatus::Network,
            format!("Could not reach GitHub ({other})."),
        ),
    }
}

fn rate_limit_hint(until: i64, now_secs: i64) -> String {
    let minutes = ((until - now_secs).max(1) + 59) / 60;
    format!("GitHub rate limit reached — polling resumes in {minutes} min.")
}

fn empty(status: GithubStatus, hint: String, settings: &AppSettings) -> GithubPrsDto {
    GithubPrsDto {
        status,
        hint: Some(hint),
        stale: false,
        viewer: None,
        fetched_at: None,
        scope: settings.github_scopes.clone(),
        mine: GithubPrListDto::default(),
        review_requested: GithubPrListDto::default(),
        assigned: GithubPrListDto::default(),
        rate_limit: None,
        warnings: Vec::new(),
    }
}

/// A failed read backed by the last successful one: the lists, viewer, scope and read time come
/// from the snapshot, the status and hint from the failure.
fn with_snapshot(failure: GithubPrsDto, snapshot_path: &Path) -> GithubPrsDto {
    match snapshot::load(snapshot_path) {
        Some(snapshot) => GithubPrsDto {
            status: failure.status,
            hint: failure.hint,
            stale: true,
            viewer: snapshot.viewer,
            fetched_at: snapshot.fetched_at,
            scope: snapshot.scope,
            mine: snapshot.mine,
            review_requested: snapshot.review_requested,
            assigned: snapshot.assigned,
            rate_limit: snapshot.rate_limit,
            warnings: Vec::new(),
        },
        None => failure,
    }
}

fn rfc3339(epoch_secs: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(epoch_secs, 0)
        .map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::AgentCli;
    use crate::cli_stub::CliStub;
    use crate::dto::GithubStatus;
    use crate::http::{refused_url, serve_sequence, CapturedRequest};
    use crate::paths::{github_prs_path_for, scratch_dir};
    use crate::settings::AppSettings;
    use fixtures::REPLY;
    use serde_json::{json, Value};
    use std::fs;
    use std::path::{Path, PathBuf};

    const NOW: i64 = 1_787_000_000;

    fn settings(scopes: &[&str], poll_seconds: u16) -> AppSettings {
        AppSettings {
            github_scopes: scopes.iter().map(|scope| scope.to_string()).collect(),
            github_poll_seconds: poll_seconds,
            ..AppSettings::default()
        }
    }

    fn gh(dir: &Path, token: &str) -> AgentCli {
        CliStub::new("gh")
            .log_args("args.txt", true)
            .stdout(token)
            .cli(dir)
    }

    fn gh_runs(dir: &Path) -> usize {
        fs::read_to_string(dir.join("args.txt"))
            .map(|log| log.lines().count())
            .unwrap_or(0)
    }

    fn missing_gh(dir: &Path) -> AgentCli {
        AgentCli::new(dir.join("gh").to_string_lossy().as_ref())
    }

    struct Harness {
        home: PathBuf,
        settings: AppSettings,
        token_memo: GhTokenMemo,
        read_memo: ReadMemo,
    }

    impl Harness {
        fn new(prefix: &str, scopes: &[&str], poll_seconds: u16) -> Self {
            Self {
                home: scratch_dir(prefix),
                settings: settings(scopes, poll_seconds),
                token_memo: GhTokenMemo::new(),
                read_memo: ReadMemo::new(),
            }
        }

        fn read(&self, cli: &AgentCli, url: &str, now_secs: i64, force: bool) -> GithubPrsDto {
            read_prs_in(
                &Sources {
                    home: &self.home,
                    settings: &self.settings,
                    token_memo: &self.token_memo,
                    read_memo: &self.read_memo,
                    cli,
                    url,
                    now_secs,
                },
                force,
            )
        }
    }

    fn authorization(request: &CapturedRequest) -> String {
        request
            .head
            .lines()
            .find_map(|line| line.strip_prefix("Authorization: "))
            .unwrap_or_default()
            .to_string()
    }

    fn reply_with_budget(remaining: u64, reset_epoch_secs: i64) -> String {
        reply_with_budget_text(remaining, &rfc3339(reset_epoch_secs).unwrap())
    }

    fn reply_with_budget_text(remaining: u64, reset_at: &str) -> String {
        let mut value: Value = serde_json::from_str(REPLY).unwrap();
        value["data"]["rateLimit"] = json!({ "remaining": remaining, "resetAt": reset_at });
        value.to_string()
    }

    fn assert_no_token(dto: &GithubPrsDto, home: &Path, token: &str) {
        let json = serde_json::to_string(dto).unwrap();
        assert!(!json.contains(token), "the DTO carries the token: {json}");
        if let Ok(raw) = fs::read_to_string(github_prs_path_for(home)) {
            assert!(
                !raw.contains(token),
                "the snapshot carries the token: {raw}"
            );
        }
    }

    #[test]
    fn a_successful_read_fills_the_lists_and_remembers_them() {
        let harness = Harness::new("gh-read-ok", &["org:acme"], 60);
        let gh = gh(&harness.home.join("cli"), "gho_t");
        let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);

        let dto = harness.read(&gh, &url, NOW, false);

        assert_eq!(dto.status, GithubStatus::Ok);
        assert_eq!(dto.hint, None);
        assert!(!dto.stale);
        assert_eq!(dto.viewer.as_deref(), Some("octocat"));
        assert_eq!(dto.fetched_at.as_deref(), Some("2026-08-17T20:53:20Z"));
        assert_eq!(dto.scope, vec!["org:acme".to_string()]);
        assert_eq!(dto.mine.items.len(), 1);
        assert_eq!(dto.review_requested.items.len(), 2);
        assert_eq!(dto.assigned.total, 0);
        assert_eq!(dto.rate_limit.as_ref().unwrap().remaining, 4998);

        let requests = server.join().unwrap();
        assert_eq!(authorization(&requests[0]), "Bearer gho_t");
        let body: Value = serde_json::from_str(&requests[0].body).unwrap();
        assert_eq!(
            body["variables"]["mine"],
            "is:pr is:open author:@me org:acme"
        );
        assert_no_token(&dto, &harness.home, "gho_t");
        assert_eq!(
            snapshot::load(&github_prs_path_for(&harness.home)),
            Some(dto)
        );
    }

    #[test]
    fn the_memory_window_ends_five_seconds_before_the_poll_interval() {
        let harness = Harness::new("gh-read-cache-edge", &[], 60);
        let gh = gh(&harness.home.join("cli"), "gho_t");
        let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
        let first = harness.read(&gh, &url, NOW, false);
        server.join().unwrap();

        assert_eq!(harness.read(&gh, &refused_url(), NOW + 54, false), first);
        let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
        assert_eq!(
            harness.read(&gh, &url, NOW + 55, false).status,
            GithubStatus::Ok
        );
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn a_fresh_read_is_served_from_memory_until_the_poll_interval_nears() {
        let harness = Harness::new("gh-read-cache", &[], 60);
        let gh = gh(&harness.home.join("cli"), "gho_t");
        let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
        let first = harness.read(&gh, &url, NOW, false);
        server.join().unwrap();

        let cached = harness.read(&gh, &refused_url(), NOW + 30, false);
        assert_eq!(cached, first, "inside the window nothing is fetched");

        let expired = harness.read(&gh, &refused_url(), NOW + 56, false);
        assert_eq!(expired.status, GithubStatus::Network);
        assert!(expired.stale, "the snapshot backs the failed refresh");
        assert_eq!(expired.mine.items, first.mine.items);
        assert_eq!(expired.fetched_at, first.fetched_at);
    }

    #[test]
    fn force_skips_the_memory_but_not_the_token_memo() {
        let harness = Harness::new("gh-read-force", &[], 60);
        let cli_dir = harness.home.join("cli");
        let gh = gh(&cli_dir, "gho_t");
        let (url_a, server_a) = serve_sequence(&[("200 OK", &[], REPLY)]);
        harness.read(&gh, &url_a, NOW, false);
        server_a.join().unwrap();

        let (url_b, server_b) = serve_sequence(&[("200 OK", &[], REPLY)]);
        let forced = harness.read(&gh, &url_b, NOW + 1, true);

        assert_eq!(forced.status, GithubStatus::Ok);
        assert_eq!(server_b.join().unwrap().len(), 1);
        assert_eq!(gh_runs(&cli_dir), 1, "`gh auth token` ran once per app run");
    }

    #[test]
    fn a_missing_gh_shows_the_snapshot_as_stale() {
        let harness = Harness::new("gh-read-missing", &["org:acme"], 60);
        let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
        let first = harness.read(&gh(&harness.home.join("cli"), "gho_t"), &url, NOW, false);
        server.join().unwrap();

        let later = Harness {
            home: harness.home.clone(),
            settings: settings(&["org:other"], 60),
            token_memo: GhTokenMemo::new(),
            read_memo: ReadMemo::new(),
        };
        let dto = later.read(
            &missing_gh(&harness.home.join("nowhere")),
            &url,
            NOW + 120,
            false,
        );

        assert_eq!(dto.status, GithubStatus::GhMissing);
        assert!(dto.stale);
        assert!(
            dto.hint.as_deref().unwrap().contains("gh auth login"),
            "{:?}",
            dto.hint
        );
        assert_eq!(dto.mine.items, first.mine.items);
        assert_eq!(dto.fetched_at, first.fetched_at);
        assert_eq!(dto.viewer, first.viewer);
        assert_eq!(
            dto.scope, first.scope,
            "the lists reflect the scope they were read with"
        );
    }

    #[test]
    fn a_signed_out_gh_says_so_without_leaking_its_stderr() {
        let harness = Harness::new("gh-read-signed-out", &[], 60);
        let gh = CliStub::new("gh")
            .stderr("no oauth token found: secret-path")
            .exit(1)
            .cli(&harness.home.join("cli"));

        let dto = harness.read(&gh, &refused_url(), NOW, false);

        assert_eq!(dto.status, GithubStatus::GhNotLoggedIn);
        assert!(!dto.stale);
        assert!(dto.mine.items.is_empty());
        assert!(
            dto.hint.as_deref().unwrap().contains("gh auth login"),
            "{:?}",
            dto.hint
        );
        let json = serde_json::to_string(&dto).unwrap();
        assert!(!json.contains("secret-path"), "{json}");
    }

    #[test]
    fn a_rejected_token_is_re_read_from_gh_once() {
        let harness = Harness::new("gh-read-401-retry", &[], 60);
        let stale_gh = gh(&harness.home.join("stale"), "gho_stale");
        let (url_a, server_a) = serve_sequence(&[("200 OK", &[], REPLY)]);
        harness.read(&stale_gh, &url_a, NOW, false);
        server_a.join().unwrap();

        let fresh_dir = harness.home.join("fresh");
        let fresh_gh = gh(&fresh_dir, "gho_fresh");
        let (url_b, server_b) = serve_sequence(&[
            ("401 Unauthorized", &[], r#"{"message":"Bad credentials"}"#),
            ("200 OK", &[], REPLY),
        ]);
        let dto = harness.read(&fresh_gh, &url_b, NOW + 1, true);

        assert_eq!(dto.status, GithubStatus::Ok);
        let requests = server_b.join().unwrap();
        assert_eq!(authorization(&requests[0]), "Bearer gho_stale");
        assert_eq!(authorization(&requests[1]), "Bearer gho_fresh");
        assert_eq!(gh_runs(&fresh_dir), 1);
    }

    #[test]
    fn a_token_rejected_twice_is_reported_and_forgotten() {
        let harness = Harness::new("gh-read-401-twice", &[], 60);
        let cli_dir = harness.home.join("cli");
        let gh = gh(&cli_dir, "gho_x");
        let (url, server) = serve_sequence(&[
            ("401 Unauthorized", &[], "{}"),
            ("401 Unauthorized", &[], "{}"),
        ]);

        let dto = harness.read(&gh, &url, NOW, false);

        assert_eq!(dto.status, GithubStatus::TokenRejected);
        assert_no_token(&dto, &harness.home, "gho_x");
        assert!(
            dto.hint.as_deref().unwrap().contains("gh auth login"),
            "{:?}",
            dto.hint
        );
        assert_eq!(server.join().unwrap().len(), 2);
        assert_eq!(gh_runs(&cli_dir), 2);
        assert_eq!(
            harness
                .token_memo
                .lookup(|| Err(auth::TokenError::NotLoggedIn)),
            Err(auth::TokenError::NotLoggedIn),
            "the rejected token is not kept"
        );
    }

    #[test]
    fn an_exhausted_rate_limit_pauses_polling_until_it_resets() {
        let harness = Harness::new("gh-read-rate-limited", &[], 60);
        let gh = gh(&harness.home.join("cli"), "gho_t");
        let reset = (NOW + 300).to_string();
        let (url, server) = serve_sequence(&[(
            "403 Forbidden",
            &[
                "X-RateLimit-Remaining: 0",
                &format!("X-RateLimit-Reset: {reset}"),
            ],
            r#"{"message":"API rate limit exceeded"}"#,
        )]);

        let dto = harness.read(&gh, &url, NOW, false);
        server.join().unwrap();
        assert_eq!(dto.status, GithubStatus::RateLimited);
        assert!(
            dto.hint.as_deref().unwrap().contains("5 min"),
            "{:?}",
            dto.hint
        );

        let paused = harness.read(&gh, &refused_url(), NOW + 10, true);
        assert_eq!(
            paused.status,
            GithubStatus::RateLimited,
            "no request while paused"
        );
        assert!(
            paused.hint.as_deref().unwrap().contains("5 min"),
            "{:?}",
            paused.hint
        );

        assert_eq!(
            harness.read(&gh, &refused_url(), NOW + 299, true).status,
            GithubStatus::RateLimited,
            "still paused one second before the reset"
        );
        let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
        let resumed = harness.read(&gh, &url, NOW + 300, false);
        assert_eq!(resumed.status, GithubStatus::Ok);
        server.join().unwrap();
    }

    #[test]
    fn rate_limit_replies_without_a_usable_reset_pause_for_one_minute() {
        let past_reset = (NOW - 10).to_string();
        let cases: [(&str, Vec<String>); 2] = [
            ("429 Too Many Requests", vec![]),
            (
                "403 Forbidden",
                vec![
                    "X-RateLimit-Remaining: 0".to_string(),
                    format!("X-RateLimit-Reset: {past_reset}"),
                ],
            ),
        ];
        for (index, (status_line, headers)) in cases.iter().enumerate() {
            let harness = Harness::new(&format!("gh-read-fallback-{index}"), &[], 60);
            let gh = gh(&harness.home.join("cli"), "gho_t");
            let headers: Vec<&str> = headers.iter().map(String::as_str).collect();
            let (url, server) = serve_sequence(&[(status_line, &headers, "{}")]);

            let dto = harness.read(&gh, &url, NOW, false);
            server.join().unwrap();
            assert_eq!(dto.status, GithubStatus::RateLimited, "{status_line}");
            assert!(
                dto.hint.as_deref().unwrap().contains("1 min"),
                "{:?}",
                dto.hint
            );
            assert_eq!(
                harness.read(&gh, &refused_url(), NOW + 30, true).status,
                GithubStatus::RateLimited,
                "{status_line}: paused for a minute"
            );
            let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
            assert_eq!(
                harness.read(&gh, &url, NOW + 60, false).status,
                GithubStatus::Ok,
                "{status_line}: resumes after a minute"
            );
            server.join().unwrap();
        }
    }

    #[test]
    fn a_nearly_spent_budget_pauses_polling_too() {
        let harness = Harness::new("gh-read-low-budget", &[], 60);
        let gh = gh(&harness.home.join("cli"), "gho_t");
        let body = reply_with_budget(10, NOW + 120);
        let (url, server) = serve_sequence(&[("200 OK", &[], &body)]);

        let first = harness.read(&gh, &url, NOW, false);
        server.join().unwrap();
        assert_eq!(first.status, GithubStatus::Ok);

        let paused = harness.read(&gh, &refused_url(), NOW + 5, true);
        assert_eq!(paused.status, GithubStatus::RateLimited);
        assert!(paused.stale);
        assert_eq!(paused.mine.items, first.mine.items);
        assert!(
            paused.hint.as_deref().unwrap().contains("2 min"),
            "{:?}",
            paused.hint
        );
    }

    #[test]
    fn the_low_budget_pause_starts_below_fifty_points() {
        let fine = Harness::new("gh-read-budget-50", &[], 60);
        let gh_fine = gh(&fine.home.join("cli"), "gho_t");
        let body = reply_with_budget(50, NOW + 600);
        let (url, server) = serve_sequence(&[("200 OK", &[], &body)]);
        assert_eq!(
            fine.read(&gh_fine, &url, NOW, false).status,
            GithubStatus::Ok
        );
        server.join().unwrap();
        let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
        assert_eq!(
            fine.read(&gh_fine, &url, NOW + 56, false).status,
            GithubStatus::Ok,
            "fifty points left is not a pause"
        );
        assert_eq!(server.join().unwrap().len(), 1);

        let low = Harness::new("gh-read-budget-49", &[], 60);
        let gh_low = gh(&low.home.join("cli"), "gho_t");
        let body = reply_with_budget(49, NOW + 600);
        let (url, server) = serve_sequence(&[("200 OK", &[], &body)]);
        assert_eq!(low.read(&gh_low, &url, NOW, false).status, GithubStatus::Ok);
        server.join().unwrap();
        let paused = low.read(&gh_low, &refused_url(), NOW + 56, false);
        assert_eq!(paused.status, GithubStatus::RateLimited);
        assert!(paused.stale);
        assert!(
            paused.hint.as_deref().unwrap().contains("10 min"),
            "{:?}",
            paused.hint
        );
    }

    #[test]
    fn an_unreadable_reset_instant_with_a_low_budget_pauses_for_one_minute() {
        let harness = Harness::new("gh-read-budget-unreadable", &[], 60);
        let gh = gh(&harness.home.join("cli"), "gho_t");
        let body = reply_with_budget_text(3, "soon");
        let (url, server) = serve_sequence(&[("200 OK", &[], &body)]);
        assert_eq!(harness.read(&gh, &url, NOW, false).status, GithubStatus::Ok);
        server.join().unwrap();
        assert_eq!(
            harness.read(&gh, &refused_url(), NOW + 30, true).status,
            GithubStatus::RateLimited
        );
        let (url, server) = serve_sequence(&[("200 OK", &[], REPLY)]);
        assert_eq!(
            harness.read(&gh, &url, NOW + 60, false).status,
            GithubStatus::Ok
        );
        server.join().unwrap();
    }

    #[test]
    fn network_and_unreadable_replies_report_network() {
        let harness = Harness::new("gh-read-network", &[], 60);
        let gh = gh(&harness.home.join("cli"), "gho_t");

        let refused = harness.read(&gh, &refused_url(), NOW, true);
        assert_eq!(refused.status, GithubStatus::Network);
        assert!(
            refused
                .hint
                .as_deref()
                .unwrap()
                .contains("Could not reach GitHub"),
            "{:?}",
            refused.hint
        );

        let (url, server) = serve_sequence(&[("200 OK", &[], "<html>")]);
        assert_eq!(
            harness.read(&gh, &url, NOW, true).status,
            GithubStatus::Network
        );
        server.join().unwrap();

        let (url, server) = serve_sequence(&[(
            "200 OK",
            &[],
            r#"{"data":null,"errors":[{"message":"Bad query"}]}"#,
        )]);
        let unreadable = harness.read(&gh, &url, NOW, true);
        server.join().unwrap();
        assert_eq!(unreadable.status, GithubStatus::Network);
        assert!(
            unreadable.hint.as_deref().unwrap().contains("Bad query"),
            "{:?}",
            unreadable.hint
        );
        assert!(
            !unreadable.stale,
            "nothing succeeded, so there is no snapshot"
        );
    }

    #[test]
    fn the_memory_window_follows_the_current_poll_interval() {
        let mut harness = Harness::new("gh-read-ttl", &[], 300);
        let gh = gh(&harness.home.join("cli"), "gho_t");
        let (url_a, server_a) = serve_sequence(&[("200 OK", &[], REPLY)]);
        harness.read(&gh, &url_a, NOW, false);
        server_a.join().unwrap();

        harness.settings = settings(&[], 30);
        let (url_b, server_b) = serve_sequence(&[("200 OK", &[], REPLY)]);
        let dto = harness.read(&gh, &url_b, NOW + 40, false);

        assert_eq!(dto.status, GithubStatus::Ok);
        assert_eq!(
            server_b.join().unwrap().len(),
            1,
            "a shorter interval shrinks the window"
        );
    }
}
