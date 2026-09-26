//! Poll every saved subscription without publishing credentials to a native client. Shared
//! native shadows are access-only. Only a never-activated isolated sign-in owns renewal.
use super::{
    model::Identity,
    store::{Guard, Login, Profile, Store, Ticket},
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
/// One saved profile's fetch: the login it was made with, which a renewal may have replaced, and
/// the reading.
pub(super) struct FetchResult {
    pub(super) login: Option<Login>,
    pub(super) result: Result<ProviderLimitsDto, SavedReadError>,
}

static ATTEMPTS: OnceLock<Mutex<HashMap<String, Attempt>>> = OnceLock::new();

/// Caller holds the provider activity read lease and shares the Limits cache with all surfaces.
/// At most two saved-account requests per provider run at once; no state mutex spans a network call.
pub(crate) fn refresh(provider: AgentId, force: bool, entries: &mut Vec<ProviderLimitsDto>) {
    #[cfg(test)]
    if tests::refresh_fixture(force, entries) {
        return;
    }
    let Ok(accounts) = super::Accounts::live() else {
        return;
    };
    accounts.refresh_usage(provider, force, entries, &fetch_profile);
}

/// How one saved profile's usage is fetched, given how to open the vault.
type Fetch<'a> = dyn Fn(&Profile, &dyn Fn() -> Result<Store, String>) -> FetchResult + Sync + 'a;

impl super::Accounts {
    /// Merges a fresh reading of every saved `provider` account but the one its CLI is signed in
    /// with into `entries`, each fetched by `fetch`. Nothing is read on a device with no vault.
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
        refresh_with(home, provider, force, entries, native, &open, &|profile| {
            fetch(profile, &open)
        });
    }
}

/// `native` is who the CLI is signed in as with a subscription; a native store that could not be
/// read reads no saved account either.
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
    // A pending recovery refuses the ticket: no saved account is polled until it is recovered.
    let Ok(ticket) = db.ticket(Guard::SignIn) else {
        return;
    };
    let profiles: Vec<_> = db
        .profiles
        .into_iter()
        .filter(|p| {
            p.identity.provider == provider
                && native.as_ref() != Some(&p.identity)
                && p.login.is_some()
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
    let (error, rejected, retry) = match &result {
        Ok(_) => (None, false, Duration::ZERO),
        Err(SavedReadError::Http(HttpError::Unauthorized)) => (
            Some(
                "Usage refresh needs sign-in again or renewal by the client that owns this login."
                    .into(),
            ),
            true,
            Duration::ZERO,
        ),
        // Renewal cannot change whose login it is, so only a new login is worth another read.
        Err(SavedReadError::OtherAccount) => (
            Some("This saved login now signs in as a different account. Sign in again.".into()),
            true,
            Duration::ZERO,
        ),
        // Not held back like a refusal: the next capture of the login this one was saved from
        // brings its renewal, and a new login is read at once.
        Err(SavedReadError::Expired) => (
            Some(
                "This saved login has expired. Use this account once, or sign in again, to renew it."
                    .into(),
            ),
            false,
            Duration::ZERO,
        ),
        Err(SavedReadError::Http(HttpError::RateLimited(reset))) => {
            let seconds = match reset {
                RateLimitReset::RetryAfter(s) => *s,
                RateLimitReset::At(at) => {
                    at.saturating_sub(chrono::Utc::now().timestamp()).max(0) as u64
                }
                RateLimitReset::Unknown => 0,
            };
            (
                Some("Usage refresh is rate limited. The last reading is retained.".into()),
                false,
                Duration::from_secs(seconds.min(86400)),
            )
        }
        Err(_) => (
            Some("Usage refresh is unavailable. The last reading is retained.".into()),
            false,
            Duration::ZERO,
        ),
    };
    // This attempt, as the next poll of the same login is held back by: until a new login when it
    // was rejected, else for the poll interval, doubled for each failure in a row.
    let record = |error: &Option<String>, rejected: bool, retry: Duration| {
        let failures = if error.is_some() {
            previous
                .as_ref()
                .map_or(1, |a| a.failures.saturating_add(1))
        } else {
            0
        };
        let interval = crate::limits_refresh::poll_interval();
        let delay = interval
            .saturating_mul(1_u32 << failures.min(4))
            .min(Duration::from_secs(3600))
            .max(retry);
        if let Ok(mut attempts) = attempts.lock() {
            attempts.insert(
                key.clone(),
                Attempt {
                    fingerprint: fingerprint.clone(),
                    next: Instant::now() + delay,
                    failures,
                    error: error.clone(),
                    rejected,
                },
            );
        }
    };
    record(&error, rejected, retry);
    // Removal, reauthentication and logout can proceed while HTTP is in flight. Recheck under
    // the vault lease and hold it only for numeric snapshot publication, never for HTTP.
    let store = open().ok()?;
    store
        .recheck(&ticket.holding(profile, fingerprint.clone()))
        .ok()?;
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
                // The reading stands, under the failure; only the snapshot is missing, so the poll
                // counts as failed and reads again once its backoff passes.
                let error = Some("Could not save the latest usage reading.".to_string());
                record(&error, false, Duration::ZERO);
                remembered.card.status = LimitsStatus::Failed;
                remembered.card.message = error;
            }
            Some(Ok(remembered.card))
        }
        Err(_) => Some(Err(error?)),
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

/// `profile`'s fetch at `now` through its provider's adapter, which asks `urls`.
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

/// `profile`'s poll into `entries`, whose card for it then says it is a saved profile Limits polls,
/// whatever the poll came to. The signed-in account's card is never touched.
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
        // Held back by its last poll, the profile shows the snapshot that poll left.
        if let Some(i) = existing {
            entries[i].saved_profile = true;
        }
        return;
    };
    let remembered = existing.map(|i| &entries[i]);
    let mut dto = match result {
        // The poll kept from the account's file as it wrote over it (`limits::remember`).
        Ok(dto) => dto,
        // A failed poll read nothing of its own: the card keeps its identity and shows what it
        // remembers under the failure.
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
