use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

pub(super) struct PerAccount<T> {
    fresh_for: Option<Duration>,
    entries: OnceLock<Mutex<HashMap<String, Entry<T>>>>,
}

struct Entry<T> {
    fresh: Option<(Instant, T)>,
    failure: Option<(Instant, u32)>,
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

pub(super) fn backoff_delay(count: u32, interval: Duration) -> Duration {
    interval
        .saturating_mul(1 << count.saturating_sub(1).min(4))
        .min(Duration::from_secs(3600))
}

#[cfg(test)]
impl<T: Clone> PerAccount<T> {
    pub(super) fn forget(&self, account: &str) {
        self.entries().remove(account);
    }

    pub(super) fn age_answer(&self, account: &str, by: Duration) {
        if let Some(entry) = self.entries().get_mut(account) {
            entry.aged_by += by;
        }
    }
}

#[cfg(test)]
mod tests;
