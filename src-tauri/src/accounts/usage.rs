use super::{
    model::Identity,
    store::{Guard, Login, Profile, Store, Ticket},
    Home,
};
use crate::{
    dto::{AgentId, LimitsStatus, ProviderLimitsDto, Reading},
    http::{HttpError, RateLimitReset},
    limits::{SavedReadError, SavedReadUrls},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Attempt {
    fingerprint: String,
    next: Instant,
    failures: u32,
    error: Option<String>,
    rejected: bool,
}
pub(super) struct FetchResult {
    pub(super) login: Option<Login>,
    pub(super) result: Result<ProviderLimitsDto, SavedReadError>,
}

static ATTEMPTS: OnceLock<Mutex<HashMap<String, Attempt>>> = OnceLock::new();

pub(crate) fn refresh(provider: AgentId, force: bool, entries: &mut Vec<ProviderLimitsDto>) {
    let Ok(accounts) = super::Accounts::live() else {
        return;
    };
    accounts.refresh_usage(provider, force, entries, &fetch_profile);
}

type Fetch<'a> = dyn Fn(&Profile, &dyn Fn() -> Result<Store, String>) -> FetchResult + Sync + 'a;

impl super::Accounts {
    pub(super) fn refresh_usage(
        &self,
        provider: AgentId,
        force: bool,
        entries: &mut Vec<ProviderLimitsDto>,
        fetch: &Fetch<'_>,
    ) {
        let home = &self.home;
        if !Store::vault_exists(home) {
            return;
        }
        let open = || Store::open_existing(home);
        let native = self
            .native(provider)
            .and_then(|native| native.subscription());
        let homes = self.account_homes(provider);
        if let (Some(homes), Ok(native)) = (&homes, &native) {
            super::homes::settle(home, provider, native.as_ref(), &open, homes);
        }
        refresh_with(
            home,
            provider,
            force,
            entries,
            native.clone(),
            &open,
            &|profile| fetch(profile, &open),
        );
        if let Some(homes) = &homes {
            refresh_homes(home, provider, force, entries, native, &open, homes);
        }
    }
}

fn refresh_with(
    home: &Path,
    provider: AgentId,
    force: bool,
    entries: &mut Vec<ProviderLimitsDto>,
    native: Result<Option<Identity>, String>,
    open: &(dyn Fn() -> Result<Store, String> + Sync),
    fetch: &(dyn Fn(&Profile) -> FetchResult + Sync),
) {
    let Ok(native) = native else {
        return;
    };
    let Ok(db) = open().and_then(|s| s.load()) else {
        return;
    };
    let Ok(ticket) = db.ticket(Guard::SignIn) else {
        return;
    };
    let archived = crate::limits::archived(home, provider);
    let profiles: Vec<_> = db
        .profiles
        .into_iter()
        .filter(|p| {
            p.identity.provider == provider
                && native.as_ref() != Some(&p.identity)
                && p.login.is_some()
                && !archived.contains(&p.identity.observation_key())
        })
        .collect();
    for batch in profiles.chunks(2) {
        let results = std::thread::scope(|scope| {
            let tasks: Vec<_> = batch
                .iter()
                .map(|profile| {
                    let (open, ticket) = (&open, &ticket);
                    scope.spawn(move || poll_with(home, profile, ticket, force, open, fetch))
                })
                .collect();
            tasks
                .into_iter()
                .map(|t| t.join().unwrap_or(None))
                .collect::<Vec<_>>()
        });
        for (profile, result) in batch.iter().zip(results) {
            merge(entries, profile, result);
        }
    }
}

fn poll_with(
    home: &Path,
    profile: &Profile,
    ticket: &Ticket,
    force: bool,
    open: &dyn Fn() -> Result<Store, String>,
    fetch: &dyn Fn(&Profile) -> FetchResult,
) -> Option<Result<ProviderLimitsDto, String>> {
    let login = profile.login.as_ref()?;
    let fingerprint = super::view(profile.identity.provider, login)
        .ok()?
        .fingerprint();
    let key = format!("{}:{}", home.display(), profile.id);
    let attempts = ATTEMPTS.get_or_init(Mutex::default);
    let previous = attempts
        .lock()
        .ok()?
        .get(&key)
        .cloned()
        .filter(|a| a.fingerprint == fingerprint);
    if let Some(previous) = &previous {
        if previous.rejected
            || (Instant::now() < previous.next && (!force || previous.error.is_some()))
        {
            open()
                .ok()?
                .recheck(&ticket.holding(profile, fingerprint))
                .ok()?;
            return previous.error.clone().map(Err);
        }
    }
    let fetched = fetch(profile);
    let fetched_login = fetched.login?;
    let fingerprint = super::view(profile.identity.provider, &fetched_login)
        .ok()?
        .fingerprint();
    let result = fetched.result;
    let mut outcome = attempt_outcome(&result);
    let held = ticket.holding(profile, fingerprint.clone());
    let published = publish(home, profile, result, &mut outcome, &|| {
        let store = open().ok()?;
        store.recheck(&held).ok()?;
        Some(store)
    });
    hold_back(attempts, key, fingerprint, previous.as_ref(), outcome);
    published
}

fn publish(
    home: &Path,
    profile: &Profile,
    result: Result<ProviderLimitsDto, SavedReadError>,
    outcome: &mut AttemptOutcome,
    lease: &dyn Fn() -> Option<Store>,
) -> Option<Result<ProviderLimitsDto, String>> {
    let _lease = lease()?;
    match result {
        Ok(mut dto) => {
            if dto
                .account
                .as_ref()
                .is_none_or(|a| a.id != profile.identity.observation_key())
            {
                return None;
            }
            if let Some(account) = &mut dto.account {
                if account.label.is_none() {
                    account.label.clone_from(&profile.email);
                }
            }
            let mut remembered = crate::limits::remember(home, dto);
            if remembered.saved.is_err() {
                *outcome = AttemptOutcome::failed(UNSAVED, false, Duration::ZERO);
                remembered.card.status = LimitsStatus::Failed;
                remembered.card.message.clone_from(&outcome.error);
            }
            Some(Ok(remembered.card))
        }
        Err(_) => outcome.error.clone().map(Err),
    }
}

fn hold_back(
    attempts: &Mutex<HashMap<String, Attempt>>,
    key: String,
    fingerprint: String,
    previous: Option<&Attempt>,
    outcome: AttemptOutcome,
) {
    let failures = if outcome.error.is_some() {
        previous.map_or(1, |a| a.failures.saturating_add(1))
    } else {
        0
    };
    let delay = crate::limits_refresh::poll_interval()
        .saturating_mul(1_u32 << failures.min(4))
        .min(Duration::from_secs(3600))
        .max(outcome.retry);
    if let Ok(mut attempts) = attempts.lock() {
        attempts.insert(
            key,
            Attempt {
                fingerprint,
                next: Instant::now() + delay,
                failures,
                error: outcome.error,
                rejected: outcome.rejected,
            },
        );
    }
}

fn refresh_homes(
    home: &Path,
    provider: AgentId,
    force: bool,
    entries: &mut Vec<ProviderLimitsDto>,
    native: Result<Option<Identity>, String>,
    open: &(dyn Fn() -> Result<Store, String> + Sync),
    homes: &super::homes::Resolve<'_>,
) {
    let Ok(native) = native else {
        return;
    };
    let Ok(db) = open().and_then(|s| s.load()) else {
        return;
    };
    let Ok(ticket) = db.ticket(Guard::SignIn) else {
        return;
    };
    let archived = crate::limits::archived(home, provider);
    let homed: Vec<(Profile, Box<dyn Home>)> = db
        .profiles
        .into_iter()
        .filter(|p| {
            p.identity.provider == provider
                && p.login.is_none()
                && native.as_ref() != Some(&p.identity)
                && !archived.contains(&p.identity.observation_key())
        })
        .filter_map(|p| {
            let resolved = homes(p.home.as_deref()?).ok()?;
            Some((p, resolved))
        })
        .collect();
    for batch in homed.chunks(2) {
        let results = std::thread::scope(|scope| {
            let tasks: Vec<_> = batch
                .iter()
                .map(|(profile, resolved)| {
                    let ticket = &ticket;
                    scope.spawn(move || {
                        poll_home(home, profile, ticket, force, open, &|profile| {
                            resolved.read_usage(&profile.identity)
                        })
                    })
                })
                .collect();
            tasks
                .into_iter()
                .map(|t| t.join().unwrap_or(None))
                .collect::<Vec<_>>()
        });
        for ((profile, _), result) in batch.iter().zip(results) {
            merge(entries, profile, result);
        }
    }
}

fn poll_home(
    home: &Path,
    profile: &Profile,
    ticket: &Ticket,
    force: bool,
    open: &dyn Fn() -> Result<Store, String>,
    read: &dyn Fn(&Profile) -> Result<ProviderLimitsDto, SavedReadError>,
) -> Option<Result<ProviderLimitsDto, String>> {
    let held = ticket.holding_home(profile)?;
    let generation = profile.home.clone()?;
    let key = format!("{}:{}", home.display(), profile.id);
    let attempts = ATTEMPTS.get_or_init(Mutex::default);
    let previous = attempts
        .lock()
        .ok()?
        .get(&key)
        .cloned()
        .filter(|a| a.fingerprint == generation);
    if let Some(previous) = &previous {
        if Instant::now() < previous.next && (!force || previous.error.is_some()) {
            open().ok()?.recheck(&held).ok()?;
            return previous.error.clone().map(Err);
        }
    }
    let result = read(profile);
    let mut outcome = home_outcome(&result);
    let published = publish(home, profile, result, &mut outcome, &|| {
        let store = open().ok()?;
        store.recheck(&held).ok()?;
        Some(store)
    });
    hold_back(attempts, key, generation, previous.as_ref(), outcome);
    published
}

fn home_outcome(result: &Result<ProviderLimitsDto, SavedReadError>) -> AttemptOutcome {
    let failed = |error: &str| AttemptOutcome::failed(error, false, Duration::ZERO);
    match result {
        Err(SavedReadError::Http(HttpError::Unauthorized)) => {
            failed("This account's saved login has ended. Sign in again.")
        }
        Err(SavedReadError::OtherAccount) => {
            failed("This account's saved login now signs in as a different account. Sign in again.")
        }
        _ => AttemptOutcome {
            rejected: false,
            ..attempt_outcome(result)
        },
    }
}

const UNSAVED: &str = "Could not save the latest usage reading.";

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttemptOutcome {
    error: Option<String>,
    rejected: bool,
    retry: Duration,
}

impl AttemptOutcome {
    fn failed(error: &str, rejected: bool, retry: Duration) -> Self {
        Self {
            error: Some(error.to_string()),
            rejected,
            retry,
        }
    }
}

fn attempt_outcome(result: &Result<ProviderLimitsDto, SavedReadError>) -> AttemptOutcome {
    match result {
        Ok(_) => AttemptOutcome {
            error: None,
            rejected: false,
            retry: Duration::ZERO,
        },
        Err(SavedReadError::Http(HttpError::Unauthorized)) => AttemptOutcome::failed(
            "Usage refresh needs sign-in again or renewal by the client that owns this login.",
            true,
            Duration::ZERO,
        ),
        Err(SavedReadError::OtherAccount) => AttemptOutcome::failed(
            "This saved login now signs in as a different account. Sign in again.",
            true,
            Duration::ZERO,
        ),
        Err(SavedReadError::Unavailable(why)) => AttemptOutcome::failed(why, false, Duration::ZERO),
        Err(SavedReadError::Http(HttpError::RateLimited(reset))) => {
            let seconds = match reset {
                RateLimitReset::RetryAfter(s) => *s,
                RateLimitReset::At(at) => {
                    at.saturating_sub(chrono::Utc::now().timestamp()).max(0) as u64
                }
                RateLimitReset::Unknown => 0,
            };
            AttemptOutcome::failed(
                "Usage refresh is rate limited. The last reading is retained.",
                false,
                Duration::from_secs(seconds.min(86400)),
            )
        }
        Err(_) => AttemptOutcome::failed(
            "Usage refresh is unavailable. The last reading is retained.",
            false,
            Duration::ZERO,
        ),
    }
}

fn fetch_profile(profile: &Profile, open: &dyn Fn() -> Result<Store, String>) -> FetchResult {
    fetch_profile_at(
        profile,
        open,
        chrono::Utc::now().timestamp_millis(),
        &SavedReadUrls::LIVE,
    )
}

fn fetch_profile_at(
    profile: &Profile,
    open: &dyn Fn() -> Result<Store, String>,
    now: i64,
    urls: &SavedReadUrls<'_>,
) -> FetchResult {
    fetch_with(
        profile,
        now,
        &|login| {
            super::adapter(profile.identity.provider)
                .map_err(|_| HttpError::Unauthorized)?
                .read_usage(&profile.identity, login, now, urls)
        },
        &|| {
            super::usage_renew::renew_owned(profile, open, &|login| {
                super::usage_renew::request(
                    profile.identity.provider,
                    login,
                    chrono::Utc::now().timestamp_millis(),
                )
            })
            .map_err(|_| {
                HttpError::Network("Private account renewal requires recovery or sign-in.".into())
                    .into()
            })
        },
    )
}

fn fetch_with(
    profile: &Profile,
    now: i64,
    read: &dyn Fn(&Login) -> Result<ProviderLimitsDto, SavedReadError>,
    renew: &dyn Fn() -> Result<Login, SavedReadError>,
) -> FetchResult {
    let mut login = profile.login.clone();
    let result = (|| {
        let current = login.as_mut().ok_or(HttpError::Unauthorized)?;
        let view =
            super::view(profile.identity.provider, current).map_err(|_| HttpError::Unauthorized)?;
        if view.identity().ok().as_ref() != Some(&profile.identity) {
            return Err(HttpError::Unauthorized.into());
        }
        let expired = view.renewal_due(now);
        drop(view);
        let mut renewed = false;
        if expired && profile.usage_renewal_owned {
            *current = renew()?;
            renewed = true;
        }
        let mut result = read(current);
        if matches!(result, Err(SavedReadError::Http(HttpError::Unauthorized)))
            && profile.usage_renewal_owned
            && !renewed
        {
            *current = renew()?;
            result = read(current);
        }
        result
    })();
    FetchResult { login, result }
}

fn merge(
    entries: &mut Vec<ProviderLimitsDto>,
    profile: &Profile,
    result: Option<Result<ProviderLimitsDto, String>>,
) {
    let key = profile.identity.observation_key();
    let existing = entries
        .iter()
        .position(|e| e.account.as_ref().is_some_and(|a| a.id == key));
    if existing.is_some_and(|i| entries[i].current_account) {
        return;
    }
    let Some(result) = result else {
        if let Some(i) = existing {
            entries[i].saved_profile = true;
        }
        return;
    };
    let remembered = existing.map(|i| &entries[i]);
    let mut dto = match result {
        Ok(dto) => dto,
        Err(error) => {
            let (provider, account, current_account) = match remembered {
                Some(card) => (card.provider, card.account.clone(), card.current_account),
                None => (
                    profile.identity.provider,
                    Some(crate::dto::LimitsAccountDto {
                        id: key,
                        label: profile.email.clone(),
                        legacy_id: None,
                    }),
                    false,
                ),
            };
            let mut dto = ProviderLimitsDto {
                provider,
                status: LimitsStatus::Failed,
                message: Some(error),
                account,
                current_account,
                saved_profile: false,
                archived: false,
                reading: Reading::default(),
            };
            if let Some(remembered) = remembered.map(|card| card.reading.clone()) {
                crate::limits::keep_remembered(&mut dto, remembered);
            }
            dto
        }
    };
    dto.saved_profile = true;
    if let Some(i) = existing {
        entries[i] = dto;
    } else {
        entries.push(dto);
    }
}

#[cfg(test)]
mod tests;
