//! The revision a process-wide cached read carries.
//!
//! Several surfaces share one cached read of the same expensive source. The Limits screen, its
//! menu-bar popover, the limits monitor and the native notch's rail all share `limits_refresh`'s
//! provider read; the Pull requests screen, the CI monitor and the notch's pull-request cell all
//! share `github`'s. Sharing keeps the provider calls down, but a consumer that also keeps its
//! own copy of the answer — the notch keeps one per cell, so it can draw while a read is in
//! flight — cannot tell a cache it has already seen from one another consumer has just replaced.
//! Left at that, a refresh the user asked for on a screen stays invisible in the notch until the
//! notch's own interval comes round, minutes later.
//!
//! A `Revision` closes that gap: it moves whenever the cached read is replaced, and a consumer
//! that records the revision its copy came from can ask "is there anything newer?" on every tick
//! without blocking on the lock a read holds for the whole of its provider call.
//!
//! The screens have the same gap in reverse — a monitor or notch read that finds fresher numbers
//! leaves them showing their own last answer until their poll interval — and they cannot poll a
//! counter from JavaScript. [`announce`] closes that side: a read that moved a revision emits
//! `shared-read-changed` to every window, and the query that owns the source refetches. That
//! refetch is served from the same cache, so it costs no provider call, and a source announces a
//! given revision once, so a cache-served refetch announces nothing and the loop stops there.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::dto::AgentId;

/// Starts at `0`, which is also the revision of a source nothing has read yet and the one
/// reported for a source that is not cached at all.
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

    /// Announces a replaced cached read and returns the revision it now carries. Call it while
    /// holding the lock that guards the read, so the revision a locked reader sees always
    /// matches the value beside it.
    pub fn bump(&self) -> u64 {
        self.0.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
    }
}

/// A shared read the windows can be told about — one variant per cache that actually exists, so
/// there is no announcing a source nothing shares. The name is the wire value the UI matches on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    LimitsClaude,
    LimitsCodex,
    GithubPrs,
}

impl Source {
    /// `None` for a provider read straight through rather than from a shared cache.
    pub fn limits(agent: AgentId) -> Option<Self> {
        match agent {
            AgentId::Claude => Some(Self::LimitsClaude),
            AgentId::Codex => Some(Self::LimitsCodex),
            AgentId::Antigravity | AgentId::Cursor => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::LimitsClaude => "limits:claude",
            Self::LimitsCodex => "limits:codex",
            Self::GithubPrs => "github:prs",
        }
    }

    /// The highest revision already announced for this source.
    fn announced(self) -> &'static AtomicU64 {
        static LIMITS_CLAUDE: AtomicU64 = AtomicU64::new(0);
        static LIMITS_CODEX: AtomicU64 = AtomicU64::new(0);
        static GITHUB_PRS: AtomicU64 = AtomicU64::new(0);
        match self {
            Self::LimitsClaude => &LIMITS_CLAUDE,
            Self::LimitsCodex => &LIMITS_CODEX,
            Self::GithubPrs => &GITHUB_PRS,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Announcement {
    source: &'static str,
    revision: u64,
}

/// Set once at setup. A read can be made from a command, a monitor or the notch supervisor, and
/// only some of those hold an `AppHandle`; keeping one here is what lets the announcement live
/// at the single point where a revision moves instead of at every call site that can move one.
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn register(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// Tells every window that `source` has a cached read newer than what they last saw. Silent when
/// this revision has already been announced, so the refetch it provokes — served from the cache,
/// carrying the same revision — does not announce again.
pub fn announce(source: Source, revision: u64) {
    // Before the watermark, so a read made before setup registered the handle leaves the
    // revision unannounced rather than marking it delivered to nobody.
    let Some(app) = APP.get() else {
        return;
    };
    if !advanced(source.announced(), revision) {
        return;
    }
    let _ = app.emit(
        "shared-read-changed",
        Announcement {
            source: source.name(),
            revision,
        },
    );
}

/// Raises the watermark to `revision` and reports whether this call is the one that raised it, so
/// concurrent readers of the same revision announce it exactly once between them.
fn advanced(announced: &AtomicU64, revision: u64) -> bool {
    announced.fetch_max(revision, Ordering::AcqRel) < revision
}

#[cfg(test)]
mod tests;
