//! The revision a process-wide value carries, and the announcement that a shared read moved.
//!
//! Several surfaces share one cached read of the same expensive source. The Limits screen, its
//! menu-bar popover, the limits monitor and the native notch's rail all share `limits_refresh`'s
//! provider read; the Pull requests screen, the CI monitor and the notch's pull-request cell all
//! share `github`'s. Sharing keeps the provider calls down, but every one of those surfaces also
//! keeps its own copy of the answer — the notch keeps one per cell so it can draw while a read is
//! in flight, the screens keep query caches — and a copy cannot tell a cache it has already seen
//! from one another surface has just replaced. Left at that, a refresh the user asked for on a
//! screen stays invisible in the notch until the notch's own interval comes round, minutes later,
//! and a monitor read that finds fresher numbers leaves the screens on their last answer.
//!
//! [`Revision`] closes the pull side: it moves once per replacement and is readable without
//! taking the lock a read holds for its whole provider call, so a consumer that recorded the
//! revision its copy came from can ask "is there anything newer?" on every tick.
//! [`announce`] closes the push side, for the windows, which cannot poll a counter from
//! JavaScript: it emits `shared-read-changed` and the query owning that source refetches.
//!
//! Two rules keep that exchange from feeding itself, and both are load-bearing:
//!
//! - **Announce a replacement, never a read.** A consumer answering an announcement re-reads, is
//!   served the value already there, and so announces nothing. That is what ends the exchange;
//!   announcing every read would need a watermark to do the same job less clearly.
//! - **Answer an announcement unforced.** A forced read bypasses the cache, which would make
//!   every announcement produce a provider call and a fresh announcement, without end. The
//!   `force` flag belongs to a person pressing refresh, never to this event.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// A monotonic generation counter for a process-wide value: it moves once each time that value is
/// replaced. Starts at `0`, which is also the revision of a value nothing has produced yet and
/// the one reported for a source that is not cached at all.
#[derive(Debug, Default)]
pub struct Revision(AtomicU64);

impl Revision {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Never blocks, so a poll loop can ask on every tick.
    pub fn current(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }

    /// Marks the value replaced and returns the revision it now carries. Call it while holding
    /// the lock that guards the value, so the revision a locked reader sees always matches what
    /// sits beside it.
    pub fn bump(&self) -> u64 {
        self.0.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
    }
}

/// What a read through a shared cache did. `Unchanged` covers every answer that left the shared
/// value where it was — served from the cache, refused while paused, or failed — because none of
/// them give another consumer something newer to pick up. Only `Replaced` is announced.
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

/// A shared read the windows can be told about — one variant per cache that actually exists, so
/// there is no announcing a source nothing shares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    LimitsClaude,
    LimitsCodex,
    GithubPrs,
}

impl Source {
    /// The wire value the UI matches on. The union it must agree with is `SharedReadSource` in
    /// `ui/src/lib/types.ts`; change the two together.
    fn name(self) -> &'static str {
        match self {
            Self::LimitsClaude => "limits:claude",
            Self::LimitsCodex => "limits:codex",
            Self::GithubPrs => "github:prs",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Announcement {
    source: &'static str,
}

/// Set once at setup. A read can be made from a command, a monitor or the notch supervisor, and
/// only some of those hold an `AppHandle`; keeping one here is what lets the announcement sit at
/// the point where the value is replaced rather than at every call site that can replace one.
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn register(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// Tells every window that `source` now holds a read newer than the one they are showing. Call it
/// only for a [`Reading::Replaced`], and only outside the lock that read holds.
pub fn announce(source: Source) {
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
mod tests;
