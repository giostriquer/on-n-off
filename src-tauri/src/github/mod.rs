//! The GitHub screen's pull requests: authored, review-requested, and assigned, each with the
//! head commit's CI rollup. Auth is borrowed from the `gh` CLI (`gh auth token`); on-n-off never
//! stores a GitHub token and never writes to GitHub. The only on-n-off writes are the last
//! successful read under `~/.on-n-off/github/`, so the screen has something to show at launch.

mod auth;
#[cfg(test)]
mod fixtures;
pub(crate) mod merge;
mod parse;
mod query;
mod snapshot;
#[cfg(test)]
#[path = "reader_tests.rs"]
mod tests;

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;

use crate::cli::AgentCli;
use crate::dto::{GithubPrsData, GithubPrsDto, GithubStatus};
use crate::http::{post_json, HttpError, RateLimitReset};
use crate::paths;
use crate::settings::{self, AppSettings};
use auth::{read_token_with, GhTokenMemo, TokenError, GH_TOKEN, TOKEN_DEADLINE};
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

/// A GitHub-side problem: the status the UI shows and the hint that goes with it.
type Failure = (GithubStatus, String);

/// Process-wide reader state: the last successful result and when it was read, plus the instant
/// until which polling is paused for rate-limit reasons.
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
/// by the last successful data when there is any. `force` skips the in-memory result but never
/// re-runs `gh auth token`; the token is only re-read after GitHub rejects it.
pub fn read_prs(force: bool) -> GithubPrsDto {
    let _guard = READ_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let settings = settings::load_settings();
    let home = match paths::user_home() {
        Ok(home) => home,
        Err(error) => {
            return problem(
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
                let paused = problem(
                    GithubStatus::RateLimited,
                    rate_limit_hint(until, sources.now_secs),
                    sources.settings,
                );
                let remembered = state
                    .cached
                    .as_ref()
                    .map(|(_, cached)| cached.data.clone())
                    .or_else(|| snapshot::load(&snapshot_path));
                return stale_or(paused, remembered);
            }
        }
    }
    match fetch(sources) {
        Ok(dto) => {
            if let Err(error) = snapshot::save(&snapshot_path, &dto.data) {
                eprintln!("could not remember the GitHub read: {error}");
            }
            sources.read_memo.state().cached = Some((sources.now_secs, dto.clone()));
            dto
        }
        Err((status, hint)) => {
            // A failure is never papered over by the result it replaced: the next poll asks
            // again, and meanwhile the last good data is shown as stale.
            let remembered = sources
                .read_memo
                .state()
                .cached
                .take()
                .map(|(_, cached)| cached.data)
                .or_else(|| snapshot::load(&snapshot_path));
            stale_or(problem(status, hint, sources.settings), remembered)
        }
    }
}

fn fetch(sources: &Sources<'_>) -> Result<GithubPrsDto, Failure> {
    let body = request_body(&sources.settings.github_scopes);
    let reply = post_with_token_retry(sources, &body)?;
    let parsed = parse::parse(&reply).map_err(|why| {
        (
            GithubStatus::Network,
            format!("GitHub sent an unreadable reply ({why})."),
        )
    })?;
    if let Some(limit) = &parsed.data.rate_limit {
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
        data: GithubPrsData {
            fetched_at: rfc3339(sources.now_secs),
            scope: sources.settings.github_scopes.clone(),
            ..parsed.data
        },
        warnings: parsed.warnings,
    })
}

/// One GraphQL request with the memoised token. A 401 means the memoised token is one `gh` has
/// since rotated (or revoked): ask `gh` again and retry once; a second 401 is reported.
fn post_with_token_retry(sources: &Sources<'_>, body: &Value) -> Result<Value, Failure> {
    let read_token = || read_token_with(sources.cli, TOKEN_DEADLINE);
    let mut token = sources
        .token_memo
        .lookup(read_token)
        .map_err(token_failure)?;
    let mut retried = false;
    loop {
        match post_json(sources.url, token.as_str(), body) {
            Ok(reply) => return Ok(reply),
            Err(HttpError::Unauthorized) if !retried => {
                sources.token_memo.clear();
                token = sources
                    .token_memo
                    .lookup(read_token)
                    .map_err(token_failure)?;
                retried = true;
            }
            Err(error) => {
                if error == HttpError::Unauthorized {
                    sources.token_memo.clear();
                }
                return Err(http_failure(sources, error));
            }
        }
    }
}

fn token_failure(error: TokenError) -> Failure {
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

fn http_failure(sources: &Sources<'_>, error: HttpError) -> Failure {
    match error {
        HttpError::Unauthorized => (
            GithubStatus::TokenRejected,
            "GitHub rejected the `gh` login — run `gh auth login` again, then refresh.".into(),
        ),
        HttpError::RateLimited(reset) => {
            let now = sources.now_secs;
            let until = match reset {
                RateLimitReset::RetryAfter(seconds) => {
                    now + i64::try_from(seconds).unwrap_or(RATE_LIMIT_FALLBACK_SECS)
                }
                RateLimitReset::At(at) if at > now => at,
                RateLimitReset::At(_) | RateLimitReset::Unknown => now + RATE_LIMIT_FALLBACK_SECS,
            };
            sources.read_memo.state().paused_until = Some(until);
            (GithubStatus::RateLimited, rate_limit_hint(until, now))
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

/// A failure with nothing to show but the scope it applies to.
fn problem(status: GithubStatus, hint: String, settings: &AppSettings) -> GithubPrsDto {
    GithubPrsDto {
        status,
        hint: Some(hint),
        stale: false,
        data: GithubPrsData {
            scope: settings.github_scopes.clone(),
            ..GithubPrsData::default()
        },
        warnings: Vec::new(),
    }
}

/// The failure backed by earlier data when there is any: its lists, viewer, scope and read time,
/// marked stale.
fn stale_or(failure: GithubPrsDto, remembered: Option<GithubPrsData>) -> GithubPrsDto {
    match remembered {
        Some(data) => GithubPrsDto {
            stale: true,
            data,
            ..failure
        },
        None => failure,
    }
}

fn rfc3339(epoch_secs: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(epoch_secs, 0)
        .map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, true))
}
