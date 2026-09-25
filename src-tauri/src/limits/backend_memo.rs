//! What a Codex login's reads of ChatGPT's backend remember per account: the last answer, for as
//! long as one stands, and after a failure when the account may be asked again. `credits_spent`
//! and `renewal` each keep one, so every such read waits and repeats the same way.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

pub(super) struct PerAccount<T> {
    /// How long an answer stands before the account is asked again; `None` asks on every read.
    fresh_for: Option<Duration>,
    entries: OnceLock<Mutex<HashMap<String, Entry<T>>>>,
}

struct Entry<T> {
    /// The last answer and when it was read.
    fresh: Option<(Instant, T)>,
    /// When the account may be asked again after a failure, and how many failed in a row.
    failure: Option<(Instant, u32)>,
    /// How much older a test says the answer is than the clock does. An `Instant` counts from
    /// boot on Windows, so a test cannot move one back by a day; it adds to the age instead.
    #[cfg(test)]
    aged_by: Duration,
}

impl<T> Default for Entry<T> {
    fn default() -> Self {
        Self {
            fresh: None,
            failure: None,
            #[cfg(test)]
            aged_by: Duration::ZERO,
        }
    }
}

impl<T> Entry<T> {
    fn age(&self, read_at: Instant) -> Duration {
        #[cfg(test)]
        let extra = self.aged_by;
        #[cfg(not(test))]
        let extra = Duration::ZERO;
        read_at.elapsed() + extra
    }
}

impl<T: Clone> PerAccount<T> {
    pub(super) const fn new(fresh_for: Option<Duration>) -> Self {
        Self {
            fresh_for,
            entries: OnceLock::new(),
        }
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<String, Entry<T>>> {
        self.entries
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// `read`, unless this account's last answer still stands or its last read failed recently.
    /// The read sits in series with the usage read, and every request is bounded by `http`'s
    /// timeout, so an endpoint that fails or hangs would otherwise add up to that timeout to every
    /// refresh. After a failure the account waits a poll interval, doubling with each further
    /// failure up to an hour, as a saved account's polling does (`accounts/usage.rs`); a success
    /// clears it. A skipped read is `None`, which the card fills from what it remembers.
    pub(super) fn read_backed_off(
        &self,
        account: &str,
        read: impl FnOnce() -> Option<T>,
    ) -> Option<T> {
        {
            let entries = self.entries();
            if let Some(entry) = entries.get(account) {
                if let (Some(fresh_for), Some((at, answer))) = (self.fresh_for, &entry.fresh) {
                    if entry.age(*at) < fresh_for {
                        return Some(answer.clone());
                    }
                }
                if entry
                    .failure
                    .is_some_and(|(until, _)| Instant::now() < until)
                {
                    return None;
                }
            }
        }
        // The lock is not held across the request.
        let answer = read();
        let mut entries = self.entries();
        let entry = entries.entry(account.to_string()).or_default();
        match &answer {
            Some(answer) => {
                entry.fresh = self.fresh_for.map(|_| (Instant::now(), answer.clone()));
                entry.failure = None;
                #[cfg(test)]
                {
                    entry.aged_by = Duration::ZERO;
                }
            }
            None => {
                let count = entry
                    .failure
                    .map_or(1, |(_, count)| count.saturating_add(1));
                let delay = backoff_delay(count, crate::limits_refresh::poll_interval());
                entry.failure = Some((Instant::now() + delay, count));
            }
        }
        answer
    }
}

/// How long an account waits after its `count`th failure in a row: one poll interval, doubling with
/// each further failure up to sixteen intervals, and never more than an hour.
pub(super) fn backoff_delay(count: u32, interval: Duration) -> Duration {
    interval
        .saturating_mul(1 << count.saturating_sub(1).min(4))
        .min(Duration::from_secs(3600))
}

#[cfg(test)]
impl<T: Clone> PerAccount<T> {
    /// Drop what is remembered about `account`, so a test starts from nothing.
    pub(super) fn forget(&self, account: &str) {
        self.entries().remove(account);
    }

    /// Backoff state for an account, as if its `count`th failure's wait had already run out.
    pub(super) fn failed_before(&self, account: &str, count: u32) {
        let expired = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        self.entries()
            .entry(account.to_string())
            .or_default()
            .failure = Some((expired, count));
    }

    pub(super) fn backoff_of(&self, account: &str) -> Option<(Instant, u32)> {
        self.entries().get(account).and_then(|entry| entry.failure)
    }

    /// Make the account's standing answer `by` older than it is.
    pub(super) fn age_answer(&self, account: &str, by: Duration) {
        if let Some(entry) = self.entries().get_mut(account) {
            entry.aged_by += by;
        }
    }
}

#[cfg(test)]
mod tests;
