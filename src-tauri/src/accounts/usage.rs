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
    Home { id: String, stamp: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum LoginKey {
    Vault(String),
    Home(String),
}

impl Generation {
    fn is_for(&self, login: &LoginKey) -> bool {
        match (self, login) {
            (Self::Vault(fingerprint), LoginKey::Vault(listed)) => fingerprint == listed,
            (Self::Home { id, .. }, LoginKey::Home(listed)) => id == listed,
            _ => false,
        }
    }
}

pub(super) fn login_key(profile: &Profile) -> Option<LoginKey> {
    match (&profile.login, profile.home.as_deref()) {
        (Some(login), _) => super::view(profile.identity.provider, login)
            .ok()
            .map(|view| LoginKey::Vault(view.fingerprint())),
        (None, Some(id)) => Some(LoginKey::Home(id.to_owned())),
        (None, None) => None,
    }
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

pub(super) enum Kept<'a> {
    Vault(&'a Login),
    Home(&'a str, Box<dyn Home>),
}

pub(super) fn kept<'a>(profile: &'a Profile, homes: Option<&Resolve<'_>>) -> Option<Kept<'a>> {
    match (&profile.login, profile.home.as_deref()) {
        (Some(login), _) => Some(Kept::Vault(login)),
        (None, Some(id)) => Some(Kept::Home(id, homes?(id).ok()?)),
        (None, None) => None,
    }
}

impl Kept<'_> {
    fn generation(&self, profile: &Profile) -> Result<Generation, String> {
        match self {
            Self::Vault(login) => {
                let fingerprint = super::view(profile.identity.provider, login)?.fingerprint();
                Ok(Generation::Vault(fingerprint))
            }
            Self::Home(id, home) => Ok(Generation::Home {
                id: (*id).to_owned(),
                stamp: home.login_stamp()?,
            }),
        }
    }

    fn held(&self, profile: &Profile, ticket: &Ticket) -> Result<Held, String> {
        let generation = self.generation(profile)?;
        let ticket = match &generation {
            Generation::Vault(fingerprint) => ticket.holding(profile, fingerprint.clone()),
            Generation::Home { .. } => ticket
                .holding_home(profile)
                .ok_or("A renewal ticket cannot hold an account home.")?,
        };
        Ok(Held { generation, ticket })
    }

    fn saved(&self) -> Saved {
        match self {
            Self::Vault(_) => Saved::Login,
            Self::Home(..) => Saved::Home,
        }
    }
}

static ATTEMPTS: OnceLock<Mutex<HashMap<String, Attempt>>> = OnceLock::new();

pub(crate) fn refresh_and_list(
    provider: AgentId,
    force: bool,
    entries: &mut Vec<ProviderLimitsDto>,
) -> Option<super::Roster> {
    let accounts = super::Accounts::live().ok()?;
    accounts.refresh_usage(provider, force, entries, &fetch_profile)
}

type Fetch<'a> = dyn Fn(&Profile, &dyn Fn() -> Result<Store, String>) -> FetchResult + Sync + 'a;

impl super::Accounts {
    pub(super) fn refresh_usage(
        &self,
        provider: AgentId,
        force: bool,
        entries: &mut Vec<ProviderLimitsDto>,
        fetch: &Fetch<'_>,
    ) -> Option<super::Roster> {
        let home = &self.home;
        if !Store::vault_exists(home) {
            return Some(super::Roster::default());
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
            .or_else(|| super::Roster::read(home, provider))
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
) -> Option<super::Roster> {
    let Reads { open, homes, fetch } = *reads;
    let native = native.ok()?;
    let db = open().and_then(|s| s.load()).ok()?;
    let Ok(ticket) = db.ticket(Guard::SignIn) else {
        return Some(super::Roster::recorded(&db, home, provider));
    };
    let archived = crate::limits::archived(home, provider);
    let due: Vec<(&Profile, Kept)> = db
        .profiles
        .iter()
        .filter(|p| {
            p.identity.provider == provider
                && native.as_ref() != Some(&p.identity)
                && !archived.contains(&p.identity.observation_key())
        })
        .filter_map(|profile| Some((profile, kept(profile, homes)?)))
        .collect();
    for batch in due.chunks(2) {
        let results = std::thread::scope(|scope| {
            let tasks: Vec<_> = batch
                .iter()
                .map(|(profile, kept)| {
                    let ticket = &ticket;
                    scope.spawn(move || poll(home, profile, kept, ticket, force, open, fetch))
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
    Some(super::Roster::recorded(&db, home, provider))
}

fn attempt_key(home: &Path, profile_id: &str) -> String {
    format!("{}:{profile_id}", home.display())
}

fn parked_attempt(home: &Path, profile_id: &str) -> Option<(Generation, Parked)> {
    let attempts = ATTEMPTS.get()?.lock().ok()?;
    let attempt = attempts.get(&attempt_key(home, profile_id))?;
    Some((attempt.generation.clone(), attempt.parked?))
}

pub(super) fn parked(home: &Path, profile: &Profile, kept: Option<&Kept<'_>>) -> Option<Parked> {
    let (parked_for, parked) = parked_attempt(home, &profile.id)?;
    match kept?.generation(profile) {
        Ok(now) => (now == parked_for).then_some(parked),
        Err(_) => Some(parked),
    }
}

pub(super) fn parked_for(home: &Path, profile_id: &str, login: &LoginKey) -> Option<Parked> {
    let (parked_for, parked) = parked_attempt(home, profile_id)?;
    parked_for.is_for(login).then_some(parked)
}

#[cfg(test)]
pub(super) fn park_for_test(home: &Path, profile_id: &str, login: &LoginKey, parked: Parked) {
    let generation = match login {
        LoginKey::Vault(fingerprint) => Generation::Vault(fingerprint.clone()),
        LoginKey::Home(id) => Generation::Home {
            id: id.clone(),
            stamp: String::new(),
        },
    };
    ATTEMPTS.get_or_init(Mutex::default).lock().unwrap().insert(
        attempt_key(home, profile_id),
        Attempt {
            generation,
            next: Instant::now(),
            failures: 0,
            error: None,
            parked: Some(parked),
        },
    );
}

struct Held {
    generation: Generation,
    ticket: Ticket,
}

type Polled = Option<Result<ProviderLimitsDto, String>>;

fn poll(
    home: &Path,
    profile: &Profile,
    kept: &Kept<'_>,
    ticket: &Ticket,
    force: bool,
    open: &dyn Fn() -> Result<Store, String>,
    fetch: &dyn Fn(&Profile) -> FetchResult,
) -> Polled {
    let key = attempt_key(home, &profile.id);
    let attempts = ATTEMPTS.get_or_init(Mutex::default);
    let Ok(previous) = attempts.lock().map(|attempts| attempts.get(&key).cloned()) else {
        return None;
    };
    let replay = |attempt: Option<&Attempt>, held: &Ticket| {
        let unchanged = open().is_ok_and(|store| store.recheck(held).is_ok());
        let error = attempt.and_then(|attempt| attempt.error.clone());
        error.filter(|_| unchanged).map(Err)
    };
    let Ok(now) = kept.held(profile, ticket) else {
        return replay(previous.as_ref(), ticket);
    };
    let previous = previous.filter(|a| a.generation == now.generation);
    if previous.as_ref().is_some_and(|a| a.holds(force)) {
        return replay(previous.as_ref(), &now.ticket);
    }
    let (after, result) = match kept {
        Kept::Vault(_) => {
            let fetched = fetch(profile);
            let login = fetched.login?;
            (Kept::Vault(&login).held(profile, ticket), fetched.result)
        }
        Kept::Home(_, account_home) => {
            let result = account_home.read_usage(&profile.identity);
            (kept.held(profile, ticket), result)
        }
    };
    let Ok(after) = after else {
        return None;
    };
    let mut outcome = verdict_of(kept.saved(), profile, &result);
    let published = publish(home, profile, result, &mut outcome, &|| {
        let store = open().ok()?;
        store.recheck(&after.ticket).ok()?;
        Some(store)
    });
    hold_back(
        attempts,
        key,
        after.generation,
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
    let attempt = Attempt {
        generation,
        next: Instant::now() + delay,
        failures,
        error,
        parked,
    };
    if let Ok(mut attempts) = attempts.lock() {
        attempts.insert(key, attempt);
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
