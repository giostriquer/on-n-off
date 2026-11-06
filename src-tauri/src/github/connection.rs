use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::DateTime;
use serde_json::Value;

use super::auth::{read_token_with, GhTokenMemo, TokenError, TOKEN_DEADLINE};
use super::parse::rate_limit;
use super::query::{EVENTS_PER_PAGE, EVENTS_URL, GRAPHQL_URL};
use crate::cli::AgentCli;
use crate::dto::GithubStatus;
use crate::http::{get_json_with, post_json, Forbidden, HttpError, RateLimitReset};

const LOW_BUDGET_POINTS: u64 = 50;
const RATE_LIMIT_FALLBACK_SECS: i64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Failure {
    pub(super) status: GithubStatus,
    pub(super) hint: String,
}

impl Failure {
    pub(super) fn new(status: GithubStatus, hint: impl Into<String>) -> Self {
        Self {
            status,
            hint: hint.into(),
        }
    }
}

pub(super) trait Github {
    fn reads_paused(&self) -> Option<Failure>;
    fn graphql(&self, body: &Value) -> Result<Value, Failure>;
    fn events(&self, viewer: &str, page: u32) -> Result<Value, Failure>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Budget {
    Graphql,
    Rest,
}

pub(super) struct HttpMemo {
    token: GhTokenMemo,
    pauses: Mutex<Pauses>,
}

struct Pauses {
    graphql: Option<i64>,
    rest: Option<i64>,
}

impl Pauses {
    fn of(&mut self, budget: Budget) -> &mut Option<i64> {
        match budget {
            Budget::Graphql => &mut self.graphql,
            Budget::Rest => &mut self.rest,
        }
    }
}

impl HttpMemo {
    pub(super) const fn new() -> Self {
        Self {
            token: GhTokenMemo::new(),
            pauses: Mutex::new(Pauses {
                graphql: None,
                rest: None,
            }),
        }
    }

    fn pauses(&self) -> MutexGuard<'_, Pauses> {
        self.pauses.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

static HTTP_MEMO: HttpMemo = HttpMemo::new();

pub(super) struct HttpGithub<'a> {
    pub(super) memo: &'a HttpMemo,
    pub(super) cli: &'a AgentCli,
    pub(super) graphql_url: &'a str,
    pub(super) events_url: &'a str,
    pub(super) now_secs: i64,
}

impl<'a> HttpGithub<'a> {
    pub(super) fn new(cli: &'a AgentCli, now_secs: i64) -> Self {
        Self {
            memo: &HTTP_MEMO,
            cli,
            graphql_url: GRAPHQL_URL,
            events_url: EVENTS_URL,
            now_secs,
        }
    }
}

impl Github for HttpGithub<'_> {
    fn reads_paused(&self) -> Option<Failure> {
        self.refusal(Budget::Graphql)
    }

    fn graphql(&self, body: &Value) -> Result<Value, Failure> {
        let reply = self.send(Budget::Graphql, |token| {
            post_json(self.graphql_url, token, body)
        })?;
        self.note_budget(&reply);
        Ok(reply)
    }

    fn events(&self, viewer: &str, page: u32) -> Result<Value, Failure> {
        let url = format!(
            "{}/{viewer}/events?per_page={EVENTS_PER_PAGE}&page={page}",
            self.events_url
        );
        self.send(Budget::Rest, |token| {
            let authorization = format!("Bearer {token}");
            get_json_with(
                &url,
                &[
                    ("Authorization", authorization.as_str()),
                    ("Accept", "application/vnd.github+json"),
                ],
                Forbidden::RateLimitWhenSaid,
            )
        })
    }
}

impl HttpGithub<'_> {
    fn refusal(&self, budget: Budget) -> Option<Failure> {
        let until = (*self.memo.pauses().of(budget)).filter(|until| self.now_secs < *until)?;
        Some(rate_limited(budget, until, self.now_secs))
    }

    fn pause(&self, budget: Budget, until: i64) {
        *self.memo.pauses().of(budget) = Some(until);
    }

    fn send(
        &self,
        budget: Budget,
        request: impl Fn(&str) -> Result<Value, HttpError>,
    ) -> Result<Value, Failure> {
        if let Some(paused) = self.refusal(budget) {
            return Err(paused);
        }
        let read_token = || read_token_with(self.cli, TOKEN_DEADLINE);
        let token = self.memo.token.lookup(read_token).map_err(token_failure)?;
        let reply = match request(token.as_str()) {
            Err(HttpError::Unauthorized) => {
                self.memo.token.clear();
                let token = self.memo.token.lookup(read_token).map_err(token_failure)?;
                request(token.as_str())
            }
            first => first,
        };
        reply.map_err(|error| {
            match error {
                HttpError::Unauthorized => self.memo.token.clear(),
                HttpError::RateLimited(reset) => self.pause(budget, self.limited_until(reset)),
                _ => {}
            }
            self.failure(budget, &error)
        })
    }

    fn failure(&self, budget: Budget, error: &HttpError) -> Failure {
        match error {
            HttpError::Unauthorized => Failure::new(
                GithubStatus::TokenRejected,
                "GitHub rejected the `gh` login — run `gh auth login` again, then refresh.",
            ),
            HttpError::RateLimited(reset) => {
                rate_limited(budget, self.limited_until(*reset), self.now_secs)
            }
            other => Failure::new(
                GithubStatus::Network,
                format!("Could not reach GitHub ({other})."),
            ),
        }
    }

    fn note_budget(&self, reply: &Value) {
        let Some(limit) = rate_limit(&reply["data"]["rateLimit"])
            .filter(|limit| limit.remaining < LOW_BUDGET_POINTS)
        else {
            return;
        };
        if let Some(until) = self.low_budget_until(&limit.reset_at) {
            self.pause(Budget::Graphql, until);
        }
    }

    fn limited_until(&self, reset: RateLimitReset) -> i64 {
        let now = self.now_secs;
        match reset {
            RateLimitReset::RetryAfter(seconds) => {
                now + i64::try_from(seconds).unwrap_or(RATE_LIMIT_FALLBACK_SECS)
            }
            RateLimitReset::At(at) if at > now => at,
            RateLimitReset::At(_) | RateLimitReset::Unknown => now + RATE_LIMIT_FALLBACK_SECS,
        }
    }

    fn low_budget_until(&self, reset_at: &str) -> Option<i64> {
        let now = self.now_secs;
        match DateTime::parse_from_rfc3339(reset_at) {
            Ok(at) => Some(at.timestamp()).filter(|at| *at > now),
            Err(_) => Some(now + RATE_LIMIT_FALLBACK_SECS),
        }
    }
}

pub(super) fn rate_limited(budget: Budget, until: i64, now_secs: i64) -> Failure {
    let minutes = ((until - now_secs).max(1) + 59) / 60;
    let resumes = match budget {
        Budget::Graphql => "polling resumes",
        Budget::Rest => "your events are read again",
    };
    Failure::new(
        GithubStatus::RateLimited,
        format!("GitHub rate limit reached — {resumes} in {minutes} min."),
    )
}

fn token_failure(error: TokenError) -> Failure {
    match error {
        TokenError::GhMissing => Failure::new(
            GithubStatus::GhMissing,
            "Install the GitHub CLI (`gh`), sign in with `gh auth login`, then refresh.",
        ),
        TokenError::NotLoggedIn => Failure::new(
            GithubStatus::GhNotLoggedIn,
            "Sign in with `gh auth login` in a terminal, then refresh.",
        ),
        TokenError::TimedOut => Failure::new(
            GithubStatus::GhNotLoggedIn,
            "`gh auth token` did not answer in time — check `gh auth status` in a terminal, then refresh.",
        ),
        TokenError::Spawn(why) => Failure::new(
            GithubStatus::GhMissing,
            format!("Could not start `gh` ({why})."),
        ),
    }
}

#[cfg(test)]
mod tests;
