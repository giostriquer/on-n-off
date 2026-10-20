use super::{
    homes::Resolve,
    model::Identity,
    store::{Guard, Login, Profile, Store, Ticket},
    Home,
};
use crate::{
    dto::{AgentId, LimitsStatus, ProviderLimitsDto, Reading},
    limits::{
        outcome::{self, Parked, Saved, SavedVerdict},
        Accepted, ReadFailure, SavedReadUrls,
    },
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Generation {
    Vault(String),
    Home { id: String, login: Option<String> },
}

#[derive(Clone)]
struct Attempt {
    generation: Generation,
    next: Instant,
    failures: u32,
    error: Option<String>,
    parked: Option<Parked>,
}

impl Attempt {
    fn holds(&self, force: bool) -> bool {
        self.parked.is_some() || (Instant::now() < self.next && (!force || self.error.is_some()))
    }
}

pub(super) struct FetchResult {
    pub(super) login: Option<Login>,
    pub(super) result: Result<Accepted, ReadFailure>,
}

static ATTEMPTS: OnceLock<Mutex<HashMap<String, Attempt>>> = OnceLock::new();

pub(crate) fn refresh(
    provider: AgentId,
    force: bool,
    entries: &mut Vec<ProviderLimitsDto>,
) -> bool {
    let Ok(accounts) = super::Accounts::live() else {
        return false;
    };
    accounts.refresh_usage(provider, force, entries, &fetch_profile)
}

type Fetch<'a> = dyn Fn(&Profile, &dyn Fn() -> Result<Store, String>) -> FetchResult + Sync + 'a;

struct Due<'a> {
    profile: &'a Profile,
    home: Option<Box<dyn Home>>,
}

impl super::Accounts {
    pub(super) fn refresh_usage(
        &self,
        provider: AgentId,
        force: bool,
        entries: &mut Vec<ProviderLimitsDto>,
        fetch: &Fetch<'_>,
    ) -> bool {
        let home = &self.home;
        if !Store::vault_exists(home) {
            return false;
        }
        let open = || Store::open_existing(home);
        let native = self
            .native(provider)
            .and_then(|native| native.subscription());
        let homes = self.account_homes(provider);
        let homes = homes.as_ref().map(|homes| homes as &Resolve<'_>);
        if let (Some(homes), Ok(native)) = (homes, &native) {
            super::homes::settle(home, provider, native.as_ref(), &open, homes);
        }
        let reads = Reads {
            open: &open,
            homes,
            fetch: &|profile| fetch(profile, &open),
        };
        refresh_profiles(home, provider, force, entries, native, &reads)
    }
}

struct Reads<'a> {
    open: &'a (dyn Fn() -> Result<Store, String> + Sync),
    homes: Option<&'a Resolve<'a>>,
    fetch: &'a (dyn Fn(&Profile) -> FetchResult + Sync),
}

fn refresh_profiles(
    home: &Path,
    provider: AgentId,
    force: bool,
    entries: &mut Vec<ProviderLimitsDto>,
    native: Result<Option<Identity>, String>,
    reads: &Reads<'_>,
) -> bool {
    let Reads { open, homes, fetch } = *reads;
    let Ok(native) = native else {
        return false;
    };
    let Ok(db) = open().and_then(|s| s.load()) else {
        return false;
    };
    let Ok(ticket) = db.ticket(Guard::SignIn) else {
        return false;
    };
    let archived = crate::limits::archived(home, provider);
    let mut due: Vec<Due> = db
        .profiles
        .iter()
        .filter(|p| {
            p.identity.provider == provider
                && native.as_ref() != Some(&p.identity)
                && !archived.contains(&p.identity.observation_key())
        })
        .filter_map(|profile| {
            let home = match &profile.login {
                Some(_) => None,
                None => Some(homes?(profile.home.as_deref()?).ok()?),
            };
            Some(Due { profile, home })
        })
        .collect();
    due.sort_by_key(|due| due.home.is_some());
    let before = parked_in(home);
    for batch in due.chunks(2) {
        let results = std::thread::scope(|scope| {
            let tasks: Vec<_> = batch
                .iter()
                .map(|due| {
                    let ticket = &ticket;
                    scope.spawn(move || match &due.home {
                        None => poll_vault(home, due.profile, ticket, force, open, fetch),
                        Some(account_home) => poll_home(
                            home,
                            due.profile,
                            ticket,
                            force,
                            open,
                            account_home.as_ref(),
                        ),
                    })
                })
                .collect();
            tasks
                .into_iter()
                .map(|t| t.join().unwrap_or(None))
                .collect::<Vec<_>>()
        });
        for (due, result) in batch.iter().zip(results) {
            merge(entries, due.profile, result);
        }
    }
    before != parked_in(home)
}

fn attempt_key(home: &Path, profile: &Profile) -> String {
    format!("{}:{}", home.display(), profile.id)
}

fn parked_in(home: &Path) -> Vec<(String, Generation, Parked)> {
    let prefix = format!("{}:", home.display());
    let Some(Ok(attempts)) = ATTEMPTS.get().map(Mutex::lock) else {
        return Vec::new();
    };
    let mut parked: Vec<_> = attempts
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .filter_map(|(key, attempt)| {
            Some((key.clone(), attempt.generation.clone(), attempt.parked?))
        })
        .collect();
    parked.sort_by(|a, b| a.0.cmp(&b.0));
    parked
}

pub(super) fn parked(
    home: &Path,
    profile: &Profile,
    homes: Option<&Resolve<'_>>,
) -> Option<Parked> {
    let attempt = ATTEMPTS
        .get()?
        .lock()
        .ok()?
        .get(&attempt_key(home, profile))?
        .clone();
    let parked = attempt.parked?;
    let now = match &profile.login {
        Some(login) => Generation::Vault(fingerprint(profile, login)?),
        None => home_generation(profile, homes?(profile.home.as_deref()?).ok()?.as_ref())?,
    };
    (now == attempt.generation).then_some(parked)
}

fn fingerprint(profile: &Profile, login: &Login) -> Option<String> {
    Some(
        super::view(profile.identity.provider, login)
            .ok()?
            .fingerprint(),
    )
}

fn home_generation(profile: &Profile, home: &dyn Home) -> Option<Generation> {
    Some(Generation::Home {
        id: profile.home.clone()?,
        login: home
            .read()
            .ok()
            .flatten()
            .and_then(|login| fingerprint(profile, &login)),
    })
}

struct Held {
    generation: Generation,
    ticket: Ticket,
}

struct Polled {
    held: Held,
    result: Result<Accepted, ReadFailure>,
}

fn poll_vault(
    home: &Path,
    profile: &Profile,
    ticket: &Ticket,
    force: bool,
    open: &dyn Fn() -> Result<Store, String>,
    fetch: &dyn Fn(&Profile) -> FetchResult,
) -> Option<Result<ProviderLimitsDto, String>> {
    let held = |login: &Login| {
        let fingerprint = fingerprint(profile, login)?;
        Some(Held {
            ticket: ticket.holding(profile, fingerprint.clone()),
            generation: Generation::Vault(fingerprint),
        })
    };
    let now = held(profile.login.as_ref()?)?;
    poll(home, profile, Saved::Login, force, open, now, &|| {
        let fetched = fetch(profile);
        Some(Polled {
            held: held(fetched.login.as_ref()?)?,
            result: fetched.result,
        })
    })
}

fn poll_home(
    home: &Path,
    profile: &Profile,
    ticket: &Ticket,
    force: bool,
    open: &dyn Fn() -> Result<Store, String>,
    account_home: &dyn Home,
) -> Option<Result<ProviderLimitsDto, String>> {
    let ticket = ticket.holding_home(profile)?;
    let held = || {
        Some(Held {
            generation: home_generation(profile, account_home)?,
            ticket: ticket.clone(),
        })
    };
    poll(home, profile, Saved::Home, force, open, held()?, &|| {
        let result = account_home.read_usage(&profile.identity);
        Some(Polled {
            held: held()?,
            result,
        })
    })
}

fn poll(
    home: &Path,
    profile: &Profile,
    saved: Saved,
    force: bool,
    open: &dyn Fn() -> Result<Store, String>,
    now: Held,
    read: &dyn Fn() -> Option<Polled>,
) -> Option<Result<ProviderLimitsDto, String>> {
    let key = attempt_key(home, profile);
    let attempts = ATTEMPTS.get_or_init(Mutex::default);
    let previous = attempts
        .lock()
        .ok()?
        .get(&key)
        .cloned()
        .filter(|a| a.generation == now.generation);
    if let Some(previous) = previous.as_ref().filter(|a| a.holds(force)) {
        open().ok()?.recheck(&now.ticket).ok()?;
        return previous.error.clone().map(Err);
    }
    let Polled { held, result } = read()?;
    let mut outcome = verdict_of(saved, profile, &result);
    let published = publish(home, profile, result, &mut outcome, &|| {
        let store = open().ok()?;
        store.recheck(&held.ticket).ok()?;
        Some(store)
    });
    hold_back(
        attempts,
        key,
        held.generation,
        previous.as_ref(),
        outcome,
        crate::limits_refresh::poll_interval(),
    );
    published
}

fn publish(
    home: &Path,
    profile: &Profile,
    result: Result<Accepted, ReadFailure>,
    outcome: &mut Option<SavedVerdict>,
    lease: &dyn Fn() -> Option<Store>,
) -> Option<Result<ProviderLimitsDto, String>> {
    let _lease = lease()?;
    match result {
        Ok(accepted) => {
            let mut dto = accepted.into_card();
            if let Some(account) = &mut dto.account {
                if account.label.is_none() {
                    account.label.clone_from(&profile.email);
                }
            }
            let mut remembered = crate::limits::remember(home, dto);
            if remembered.saved.is_err() {
                let unsaved = SavedVerdict {
                    message: UNSAVED.to_string(),
                    parks: None,
                    retry: Duration::ZERO,
                };
                remembered.card.status = LimitsStatus::Failed;
                remembered.card.message = Some(unsaved.message.clone());
                *outcome = Some(unsaved);
            }
            Some(Ok(remembered.card))
        }
        Err(_) => outcome.as_ref().map(|verdict| Err(verdict.message.clone())),
    }
}

fn hold_back(
    attempts: &Mutex<HashMap<String, Attempt>>,
    key: String,
    generation: Generation,
    previous: Option<&Attempt>,
    outcome: Option<SavedVerdict>,
    interval: Duration,
) {
    let failures = match outcome {
        Some(_) => previous.map_or(1, |a| a.failures.saturating_add(1)),
        None => 0,
    };
    let retry = outcome
        .as_ref()
        .map_or(Duration::ZERO, |verdict| verdict.retry);
    let delay = crate::limits_refresh::failure_backoff(interval, failures).max(retry);
    let (error, parked) = outcome.map_or((None, None), |verdict| {
        (Some(verdict.message), verdict.parks)
    });
    if let Ok(mut attempts) = attempts.lock() {
        attempts.insert(
            key,
            Attempt {
                generation,
                next: Instant::now() + delay,
                failures,
                error,
                parked,
            },
        );
    }
}

const UNSAVED: &str = "Could not save the latest usage reading.";

fn verdict_of(
    saved: Saved,
    profile: &Profile,
    result: &Result<Accepted, ReadFailure>,
) -> Option<SavedVerdict> {
    let failure = result.as_ref().err()?;
    Some(outcome::saved(
        profile.identity.provider,
        saved,
        failure,
        chrono::Utc::now().timestamp(),
    ))
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
                .map_err(|_| ReadFailure::LoginRefused)?
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
                ReadFailure::Unavailable(
                    "Private account renewal requires recovery or sign-in.".to_string(),
                )
            })
        },
    )
}

fn fetch_with(
    profile: &Profile,
    now: i64,
    read: &dyn Fn(&Login) -> Result<Accepted, ReadFailure>,
    renew: &dyn Fn() -> Result<Login, ReadFailure>,
) -> FetchResult {
    let mut login = profile.login.clone();
    let result = (|| {
        let current = login.as_mut().ok_or(ReadFailure::LoginRefused)?;
        let view = super::view(profile.identity.provider, current)
            .map_err(|_| ReadFailure::LoginRefused)?;
        if view.identity().ok().as_ref() != Some(&profile.identity) {
            return Err(ReadFailure::LoginRefused);
        }
        let expired = view.renewal_due(now);
        drop(view);
        let mut renewed = false;
        if expired && profile.usage_renewal_owned {
            *current = renew()?;
            renewed = true;
        }
        let mut result = read(current);
        if matches!(result, Err(ReadFailure::LoginRefused))
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
                    Some(crate::limits::scoped_account(
                        &profile.identity,
                        profile.email.clone(),
                    )),
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
pub(super) mod tests;
