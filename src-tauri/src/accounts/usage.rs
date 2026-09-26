//! Poll every saved subscription without publishing credentials to a native client. Shared
//! native shadows are access-only. Only a never-activated isolated sign-in owns renewal.
use super::{
    native::NativeStore,
    store::{Guard, Login, Profile, Store, Ticket},
    transaction::Native,
};
use crate::{
    dto::{AgentId, LimitsStatus, ProviderLimitsDto, Reading},
    http::{HttpError, RateLimitReset},
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
struct FetchResult {
    login: Option<Login>,
    result: Result<ProviderLimitsDto, HttpError>,
}

static ATTEMPTS: OnceLock<Mutex<HashMap<String, Attempt>>> = OnceLock::new();

/// Caller holds the provider activity read lease and shares the Limits cache with all surfaces.
/// At most two saved-account requests per provider run at once; no state mutex spans a network call.
pub(crate) fn refresh(provider: AgentId, force: bool, entries: &mut Vec<ProviderLimitsDto>) {
    #[cfg(test)]
    if tests::refresh_fixture(force, entries) {
        return;
    }
    let Ok(home) = super::home() else {
        return;
    };
    if !Store::vault_exists(&home) {
        return;
    }
    let open = || Store::open_existing(&home);
    let native = NativeStore::resolve(provider, &home).and_then(|n| n.read());
    refresh_with(&home, provider, force, entries, native, &open, &|profile| {
        fetch_profile(profile, &open)
    });
}

fn refresh_with(
    home: &Path,
    provider: AgentId,
    force: bool,
    entries: &mut Vec<ProviderLimitsDto>,
    native: Result<Option<Login>, String>,
    open: &(dyn Fn() -> Result<Store, String> + Sync),
    fetch: &(dyn Fn(&Profile) -> FetchResult + Sync),
) {
    let native = native.and_then(|login| match login {
        Some(login)
            if provider == AgentId::Codex
                && login
                    .auth
                    .get("OPENAI_API_KEY")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|key| !key.trim().is_empty()) =>
        {
            Ok(None)
        }
        Some(login) => super::view(provider, &login)
            .and_then(|login| login.identity())
            .map(Some),
        None => Ok(None),
    });
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
                .recheck(&ticket.holding(profile, login).ok()?)
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
        Err(HttpError::Unauthorized) => (
            Some(
                "Usage refresh needs sign-in again or renewal by the client that owns this login."
                    .into(),
            ),
            true,
            Duration::ZERO,
        ),
        Err(HttpError::RateLimited(reset)) => {
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
            key,
            Attempt {
                fingerprint: fingerprint.clone(),
                next: Instant::now() + delay,
                failures,
                error: error.clone(),
                rejected,
            },
        );
    }
    // Removal, reauthentication and logout can proceed while HTTP is in flight. Recheck under
    // the vault lease and hold it only for numeric snapshot publication, never for HTTP.
    let store = open().ok()?;
    store
        .recheck(&ticket.holding(profile, &fetched_login).ok()?)
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
            if crate::limits::login::remember(home, &dto).is_err() {
                return Some(Err("Could not save the latest usage reading.".into()));
            }
            Some(Ok(dto))
        }
        Err(_) => Some(Err(error?)),
    }
}

fn fetch_profile(profile: &Profile, open: &dyn Fn() -> Result<Store, String>) -> FetchResult {
    fetch_with(
        profile,
        chrono::Utc::now().timestamp_millis(),
        &|login| crate::limits::saved::read(&profile.identity, &login.auth),
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
            })
        },
    )
}

fn fetch_with(
    profile: &Profile,
    now: i64,
    read: &dyn Fn(&Login) -> Result<ProviderLimitsDto, HttpError>,
    renew: &dyn Fn() -> Result<Login, HttpError>,
) -> FetchResult {
    let mut login = profile.login.clone();
    let result = (|| {
        let current = login.as_mut().ok_or(HttpError::Unauthorized)?;
        let view =
            super::view(profile.identity.provider, current).map_err(|_| HttpError::Unauthorized)?;
        if view.identity().ok().as_ref() != Some(&profile.identity) {
            return Err(HttpError::Unauthorized);
        }
        let expired = view.renewal_due(now);
        drop(view);
        let mut renewed = false;
        if expired && profile.usage_renewal_owned {
            *current = renew()?;
            renewed = true;
        }
        let mut result = read(current);
        if matches!(result, Err(HttpError::Unauthorized)) && profile.usage_renewal_owned && !renewed
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
    let Some(result) = result else {
        return;
    };
    let key = profile.identity.observation_key();
    let existing = entries
        .iter()
        .position(|e| e.account.as_ref().is_some_and(|a| a.id == key));
    if existing.is_some_and(|i| entries[i].current_account) {
        return;
    }
    let remembered = existing.map(|i| &entries[i]);
    let mut dto = match result {
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
            ProviderLimitsDto {
                provider,
                status: LimitsStatus::Failed,
                message: Some(error),
                account,
                current_account,
                reading: Reading::default(),
            }
        }
    };
    if let Some(remembered) = remembered.map(|card| card.reading.clone()) {
        crate::limits::keep_remembered(&mut dto, remembered);
    }
    if let Some(i) = existing {
        entries[i] = dto;
    } else {
        entries.push(dto);
    }
}

#[cfg(test)]
mod tests;
