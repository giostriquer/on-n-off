//! Poll every saved subscription without publishing credentials to a native client. Shared
//! native shadows are access-only. Only a never-activated isolated sign-in owns renewal.
use super::{
    model,
    native::NativeStore,
    store::{Login, Profile, Store},
    transaction::Native,
};
use crate::{
    dto::{AgentId, LimitsCreditsSpentDto, LimitsStatus, ProviderLimitsDto},
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

/// What a login has spent lately (`limits::saved::codex_credits_spent`), injectable for tests.
type SpentReader<'a> =
    dyn Fn(&model::Identity, &serde_json::Value) -> Option<LimitsCreditsSpentDto> + 'a;

/// How `refresh_with` reads, so tests can stand in for the vault, the saved polls and the
/// signed-in login's spending without touching the network.
#[derive(Clone, Copy)]
struct Readers<'a> {
    open: &'a (dyn Fn() -> Result<Store, String> + Sync),
    fetch: &'a (dyn Fn(&Profile) -> FetchResult + Sync),
    spent: &'a SpentReader<'a>,
}

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
    if !home.join(".on-n-off/accounts/vault.enc").exists() {
        return;
    }
    let open = || Store::open_read(&home);
    let native = NativeStore::resolve(provider, &home).and_then(|n| n.read());
    refresh_with(
        &home,
        provider,
        force,
        entries,
        native,
        &Readers {
            open: &open,
            fetch: &|profile| fetch_profile(&home, profile, &open),
            spent: &crate::limits::saved::codex_credits_spent,
        },
    );
}

fn refresh_with(
    home: &Path,
    provider: AgentId,
    force: bool,
    entries: &mut Vec<ProviderLimitsDto>,
    native: Result<Option<Login>, String>,
    readers: &Readers<'_>,
) {
    let Readers { open, fetch, spent } = *readers;
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
        Some(login) => model::identity(provider, &login.auth, &login.account)
            .map(|identity| Some((identity, login))),
        None => Ok(None),
    });
    let Ok(native) = native else {
        return;
    };
    let Ok(db) = open().and_then(|s| s.load()) else {
        return;
    };
    if db.recovery.is_some() {
        return;
    }
    if let Some((identity, login)) = &native {
        attach_credits_spent(entries, identity, &login.auth, spent);
    }
    let native = native.map(|(identity, _)| identity);
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
                    let open = &open;
                    scope
                        .spawn(move || poll_with(home, profile, db.login_epoch, force, open, fetch))
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

/// app-server reads the signed-in card without a token, and the saved shadow of the same login is
/// never polled, so a workspace card is asked what it spent with the native login itself: access
/// only, like every saved read. The card is the one keyed by that login's identity.
fn attach_credits_spent(
    entries: &mut [ProviderLimitsDto],
    identity: &model::Identity,
    auth: &serde_json::Value,
    spent: &SpentReader<'_>,
) {
    if identity.provider != AgentId::Codex {
        return;
    }
    let key = identity.observation_key();
    let Some(card) = entries
        .iter_mut()
        .find(|e| e.current_account && e.account.as_ref().is_some_and(|a| a.id == key))
    else {
        return;
    };
    if card
        .plan
        .as_deref()
        .is_some_and(crate::limits::credits_spent::is_workspace_plan)
    {
        card.credits_spent = spent(identity, auth);
    }
}

fn poll_with(
    home: &Path,
    profile: &Profile,
    epoch: u64,
    force: bool,
    open: &dyn Fn() -> Result<Store, String>,
    fetch: &dyn Fn(&Profile) -> FetchResult,
) -> Option<Result<ProviderLimitsDto, String>> {
    let login = profile.login.as_ref()?;
    let fingerprint = login.fingerprint();
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
            let store = open().ok()?;
            let db = store.load().ok()?;
            if db.allow_publication(epoch).is_err()
                || !db.profiles.iter().any(|p| {
                    p.id == profile.id
                        && p.identity == profile.identity
                        && p.login
                            .as_ref()
                            .is_some_and(|l| l.fingerprint() == fingerprint)
                })
            {
                return None;
            }
            return previous.error.clone().map(Err);
        }
    }
    let fetched = fetch(profile);
    let fingerprint = fetched.login.as_ref()?.fingerprint();
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
    let db = store.load().ok()?;
    let current = db
        .profiles
        .iter()
        .find(|p| p.id == profile.id && p.identity == profile.identity)?;
    if db.allow_publication(epoch).is_err() || current.login.as_ref()?.fingerprint() != fingerprint
    {
        return None;
    }
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

fn fetch_profile(
    home: &Path,
    profile: &Profile,
    open: &dyn Fn() -> Result<Store, String>,
) -> FetchResult {
    fetch_with(
        profile,
        chrono::Utc::now().timestamp_millis(),
        &|login| crate::limits::saved::read(&profile.identity, &login.auth),
        &|| {
            super::usage_renew::renew_owned(home, profile, open, &|login| {
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
        if model::identity(profile.identity.provider, &current.auth, &current.account)
            .ok()
            .as_ref()
            != Some(&profile.identity)
        {
            return Err(HttpError::Unauthorized);
        }
        let expired = match profile.identity.provider {
            AgentId::Claude => current
                .auth
                .pointer("/claudeAiOauth/expiresAt")
                .and_then(serde_json::Value::as_i64)
                .is_some_and(|v| v <= now),
            AgentId::Codex => model::codex_renews_soon(&current.auth, now / 1000),
            _ => false,
        };
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
    let dto = match result {
        Ok(mut dto) => {
            if let Some(i) = existing {
                dto.keep_reset_credits_from(&entries[i]);
            }
            dto
        }
        Err(error) => {
            let mut dto =
                existing
                    .map(|i| entries[i].clone())
                    .unwrap_or_else(|| ProviderLimitsDto {
                        provider: profile.identity.provider,
                        status: LimitsStatus::Failed,
                        message: None,
                        account: Some(crate::dto::LimitsAccountDto {
                            id: key,
                            label: profile.email.clone(),
                            legacy_id: None,
                        }),
                        current_account: false,
                        plan: None,
                        subscription_status: None,
                        windows: vec![],
                        credits: None,
                        workspace_credits: None,
                        credits_spent: None,
                        reset_credits: None,
                        reset_offer: None,
                    });
            dto.status = LimitsStatus::Failed;
            dto.message = Some(error);
            dto
        }
    };
    if let Some(i) = existing {
        entries[i] = dto;
    } else {
        entries.push(dto);
    }
}

#[cfg(test)]
mod tests;
