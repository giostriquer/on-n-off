mod auth;
mod connection;
mod contributions;
#[cfg(test)]
mod fixtures;
pub(crate) mod merge;
mod parse;
mod query;
mod snapshot;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, SecondsFormat, Utc};

use crate::cli::AgentCli;
use crate::dto::{GithubPrDto, GithubPrsData, GithubPrsDto, GithubStatus};
use crate::paths;
use crate::read_revision::{Reading, Revision, Source};
use crate::settings::{self, AppSettings};
use connection::{Failure, Github, HttpGithub};
use merge::KnownAnswers;
use query::request_body;

const CACHE_MARGIN_SECS: i64 = 5;

static READ_LOCK: Mutex<()> = Mutex::new(());
static READ_MEMO: ReadMemo = ReadMemo::new();
pub(crate) static POLL_SECONDS: AtomicU64 = AtomicU64::new(0);

pub struct ReadMemo {
    state: Mutex<ReadMemoState>,
    revision: Revision,
}

#[derive(Default)]
struct ReadMemoState {
    cached: Option<(i64, GithubPrsDto)>,
    known_answers: Option<KnownAnswers>,
}

impl ReadMemo {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(ReadMemoState {
                cached: None,
                known_answers: None,
            }),
            revision: Revision::new(),
        }
    }

    fn state(&self) -> MutexGuard<'_, ReadMemoState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn fresh(&self, now_secs: i64, window: i64) -> Option<GithubPrsDto> {
        let state = self.state();
        let (read_at, cached) = state.cached.as_ref()?;
        (now_secs - read_at < window).then(|| cached.clone())
    }
}

struct Sources<'a> {
    home: &'a Path,
    settings: &'a AppSettings,
    read_memo: &'a ReadMemo,
    github: &'a dyn Github,
    now_secs: i64,
}

pub fn read_prs(force: bool) -> GithubPrsDto {
    read_prs_revisioned(force).0
}

pub fn read_prs_revisioned(force: bool) -> (GithubPrsDto, u64) {
    let (dto, reading) = read_prs_locked(force);
    if reading.replaced() {
        crate::read_revision::announce(Source::GithubPrs);
    }
    (dto, reading.revision())
}

pub fn set_poll_seconds(seconds: u16) {
    POLL_SECONDS.store(u64::from(seconds), Ordering::Release);
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn poll_interval_in(memo: &AtomicU64) -> std::time::Duration {
    settings::remembered_seconds(memo, || {
        u64::from(settings::load_settings().github_poll_seconds)
    })
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn revision() -> u64 {
    READ_MEMO.revision.current()
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
pub(crate) fn bump_revision() -> u64 {
    READ_MEMO.revision.bump()
}

fn read_prs_locked(force: bool) -> (GithubPrsDto, Reading) {
    let _guard = READ_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let settings = settings::load_settings();
    let home = match paths::user_home() {
        Ok(home) => home,
        Err(error) => {
            return (
                problem(
                    Failure::new(
                        GithubStatus::Network,
                        format!("Could not find the home directory: {}", error.message),
                    ),
                    &settings,
                ),
                Reading::Unchanged(READ_MEMO.revision.current()),
            )
        }
    };
    let cli = AgentCli::new("gh");
    let now_secs = Utc::now().timestamp();
    read_prs_in(
        &Sources {
            home: &home,
            settings: &settings,
            read_memo: &READ_MEMO,
            github: &HttpGithub::new(&cli, now_secs),
            now_secs,
        },
        force,
    )
}

fn read_prs_in(sources: &Sources<'_>, force: bool) -> (GithubPrsDto, Reading) {
    let window = i64::from(sources.settings.github_poll_seconds) - CACHE_MARGIN_SECS;
    let snapshot_path = paths::github_prs_path_for(sources.home);
    if !force {
        if let Some(cached) = sources.read_memo.fresh(sources.now_secs, window) {
            return (
                cached,
                Reading::Unchanged(sources.read_memo.revision.current()),
            );
        }
    }
    if let Some(paused) = sources.github.reads_paused() {
        let cached = sources
            .read_memo
            .state()
            .cached
            .as_ref()
            .map(|(_, cached)| cached.data.clone());
        let remembered =
            cached.or_else(|| snapshot::load(&snapshot_path).map(|snapshot| snapshot.data));
        return (
            stale_or(problem(paused, sources.settings), remembered),
            Reading::Unchanged(sources.read_memo.revision.current()),
        );
    }
    let known_answers = sources.read_memo.state().known_answers.take();
    let mut known_answers =
        known_answers.unwrap_or_else(|| remembered_answers(sources.home, &snapshot_path));
    match fetch(sources, &mut known_answers) {
        Ok(dto) => {
            if let Err(error) = snapshot::save(&snapshot_path, &dto.data, &known_answers) {
                eprintln!("could not remember the GitHub read: {error}");
            }
            let mut state = sources.read_memo.state();
            state.cached = Some((sources.now_secs, dto.clone()));
            state.known_answers = Some(known_answers);
            let reading = Reading::Replaced(sources.read_memo.revision.bump());
            drop(state);
            (dto, reading)
        }
        Err(failure) => {
            let remembered = {
                let mut state = sources.read_memo.state();
                state.known_answers = Some(known_answers);
                state.cached.take().map(|(_, cached)| cached.data)
            }
            .or_else(|| snapshot::load(&snapshot_path).map(|snapshot| snapshot.data));
            (
                stale_or(problem(failure, sources.settings), remembered),
                Reading::Unchanged(sources.read_memo.revision.current()),
            )
        }
    }
}

fn remembered_answers(home: &Path, snapshot_path: &Path) -> KnownAnswers {
    KnownAnswers::recall(
        snapshot::load(snapshot_path),
        contributions::recorded_open(&paths::github_contributions_path_for(home)),
    )
}

fn fetch(sources: &Sources<'_>, known_answers: &mut KnownAnswers) -> Result<GithubPrsDto, Failure> {
    let reply = sources
        .github
        .graphql(&request_body(&sources.settings.github_scopes))?;
    let parsed = parse::parse(&reply).map_err(|why| {
        Failure::new(
            GithubStatus::Network,
            format!("GitHub sent an unreadable reply ({why})."),
        )
    })?;
    let mut data = GithubPrsData {
        fetched_at: rfc3339(sources.now_secs),
        scope: sources.settings.github_scopes.clone(),
        ..parsed.data
    };
    let mut warnings = parsed.warnings;
    let (unlisted, earlier) = add_contributions(sources, &mut data, &mut warnings);
    known_answers.settle(&mut data, unlisted, &earlier);
    Ok(GithubPrsDto {
        status: GithubStatus::Ok,
        hint: None,
        stale: false,
        data,
        warnings,
    })
}

fn add_contributions(
    sources: &Sources<'_>,
    data: &mut GithubPrsData,
    warnings: &mut Vec<String>,
) -> (Vec<GithubPrDto>, HashSet<String>) {
    let Some(viewer) = data
        .viewer
        .clone()
        .filter(|login| settings::is_github_login(login))
    else {
        return (Vec::new(), HashSet::new());
    };
    let found = contributions::read(
        sources.github,
        &paths::github_contributions_path_for(sources.home),
        &viewer,
        &sources.settings.github_scopes,
        sources.now_secs,
    );
    warnings.extend(found.warnings);
    contributions::add_to(data, found.open, found.merged);
    (found.unlisted, found.earlier)
}

fn problem(failure: Failure, settings: &AppSettings) -> GithubPrsDto {
    GithubPrsDto {
        status: failure.status,
        hint: Some(failure.hint),
        stale: false,
        data: GithubPrsData {
            scope: settings.github_scopes.clone(),
            ..GithubPrsData::default()
        },
        warnings: Vec::new(),
    }
}

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
