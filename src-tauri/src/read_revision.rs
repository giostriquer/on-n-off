use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Default)]
pub struct Revision(AtomicU64);

impl Revision {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    pub fn current(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }

    pub fn bump(&self) -> u64 {
        self.0.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    Unchanged(u64),
    Replaced(u64),
}

impl Reading {
    pub fn revision(self) -> u64 {
        match self {
            Self::Unchanged(revision) | Self::Replaced(revision) => revision,
        }
    }

    pub fn replaced(self) -> bool {
        matches!(self, Self::Replaced(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Accounts,
    LimitsClaude,
    LimitsCodex,
    GithubPrs,
    ResetSpends,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::Accounts => "accounts",
            Self::LimitsClaude => "limits:claude",
            Self::LimitsCodex => "limits:codex",
            Self::GithubPrs => "github:prs",
            Self::ResetSpends => "limits:reset-spends",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Announcement {
    source: &'static str,
}

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn register(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

pub fn announce(source: Source) {
    #[cfg(test)]
    ANNOUNCED.with(|announced| announced.borrow_mut().push(source));
    if let Some(app) = APP.get() {
        let _ = app.emit(
            "shared-read-changed",
            Announcement {
                source: source.name(),
            },
        );
    }
}

#[cfg(test)]
thread_local! {
    static ANNOUNCED: std::cell::RefCell<Vec<Source>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub fn take_announced() -> Vec<Source> {
    ANNOUNCED.with(std::cell::RefCell::take)
}

#[cfg(test)]
mod tests;
