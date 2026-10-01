use super::{model::Identity, transaction::Recovery};
use crate::file_lease::FileLease;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Login {
    pub auth: Value,
    pub account: Value,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub identity: Identity,
    pub label: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    pub saved_at: String,
    pub login: Option<Login>,
    #[serde(default)]
    pub pending_activation: bool,
    #[serde(default)]
    pub usage_renewal_owned: bool,
    #[serde(default)]
    pub home: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub signed_out: bool,
}
impl Profile {
    pub fn needs_login(&self) -> bool {
        self.login.is_none() && self.home.is_none()
    }
}
#[derive(Default, Serialize, Deserialize)]
pub struct Database {
    pub profiles: Vec<Profile>,
    #[serde(default)]
    login_epoch: u64,
    #[serde(default)]
    pub ignored_accounts: Vec<Identity>,
    #[serde(default)]
    pub ignored_credentials: Vec<String>,
    #[serde(default)]
    recovery: Option<Recovery>,
}

const RECOVER_FIRST: &str = "Recover the interrupted account change first.";
const SIGN_IN_CHANGED: &str = "The account state changed while sign-in was running. Start sign-in again after recovery or account changes finish.";

pub enum ChangeKind<'t> {
    Account,
    SignIn(&'t Ticket),
    Recovery,
    Remembering,
    Metadata,
}

impl ChangeKind<'_> {
    fn gate(&self, db: &Database) -> Result<(), String> {
        match self {
            Self::Account if db.recovery.is_some() => Err(RECOVER_FIRST.into()),
            Self::Recovery if db.recovery.is_none() => Err("No recovery is pending.".into()),
            Self::SignIn(ticket) => db.check(ticket),
            Self::Account | Self::Recovery | Self::Remembering | Self::Metadata => Ok(()),
        }
    }
    fn bumps(&self) -> bool {
        match self {
            Self::Account | Self::SignIn(_) | Self::Recovery | Self::Remembering => true,
            Self::Metadata => false,
        }
    }
}

#[derive(Clone)]
pub struct Ticket(Guarded);
#[derive(Clone)]
enum Guarded {
    Epoch(u64),
    Reading { epoch: u64, held: HeldLogin },
    HomeReading { epoch: u64, held: HeldHome },
    Renewal(HeldLogin),
}
#[derive(Clone)]
struct HeldLogin {
    id: String,
    identity: Identity,
    fingerprint: String,
}
impl HeldLogin {
    fn of(profile: &Profile, fingerprint: String) -> Self {
        Self {
            id: profile.id.clone(),
            identity: profile.identity.clone(),
            fingerprint,
        }
    }
    fn in_vault<'db>(&self, db: &'db Database) -> Option<&'db Profile> {
        db.profiles.iter().find(|profile| {
            profile.id == self.id
                && profile.identity == self.identity
                && profile
                    .login
                    .as_ref()
                    .and_then(|login| super::view(profile.identity.provider, login).ok())
                    .is_some_and(|login| login.fingerprint() == self.fingerprint)
        })
    }
}

#[derive(Clone)]
struct HeldHome {
    id: String,
    identity: Identity,
    home: String,
}
impl HeldHome {
    fn in_vault<'db>(&self, db: &'db Database) -> Option<&'db Profile> {
        db.profiles.iter().find(|profile| {
            profile.id == self.id
                && profile.identity == self.identity
                && profile.home.as_deref() == Some(self.home.as_str())
                && profile.login.is_none()
        })
    }
}

pub enum Guard<'a> {
    SignIn,
    Renewal(&'a Profile),
}

impl Ticket {
    pub fn holding(&self, profile: &Profile, fingerprint: String) -> Self {
        let held = HeldLogin::of(profile, fingerprint);
        Self(match self.0 {
            Guarded::Epoch(epoch)
            | Guarded::Reading { epoch, .. }
            | Guarded::HomeReading { epoch, .. } => Guarded::Reading { epoch, held },
            Guarded::Renewal(_) => Guarded::Renewal(held),
        })
    }
    pub fn holding_home(&self, profile: &Profile) -> Option<Self> {
        let held = HeldHome {
            id: profile.id.clone(),
            identity: profile.identity.clone(),
            home: profile.home.clone()?,
        };
        match self.0 {
            Guarded::Epoch(epoch)
            | Guarded::Reading { epoch, .. }
            | Guarded::HomeReading { epoch, .. } => {
                Some(Self(Guarded::HomeReading { epoch, held }))
            }
            Guarded::Renewal(_) => None,
        }
    }
}
fn login_email(provider: crate::dto::AgentId, login: &Login) -> Option<String> {
    super::view(provider, login).ok()?.email()
}
impl Database {
    pub fn observed(&self, provider: crate::dto::AgentId, key: &str) -> Option<&Profile> {
        self.profiles
            .iter()
            .find(|p| p.identity.provider == provider && p.identity.observation_key() == key)
    }
    pub fn reenroll(&mut self, identity: &Identity) {
        self.ignored_accounts.retain(|ignored| ignored != identity);
    }
    pub fn capture_native(&mut self, identity: Identity, login: Login) -> Result<(), String> {
        if self.ignored_accounts.contains(&identity)
            || self
                .profiles
                .iter()
                .any(|p| p.identity == identity && p.pending_activation)
        {
            return Ok(());
        }
        self.save(identity, login, None).map(|_| ())
    }

    pub fn set_category(&mut self, id: &str, value: &str) -> Result<(), String> {
        let value = value.trim();
        if value.chars().count() > 100 {
            return Err("Keep the category within 100 characters.".into());
        }
        let profile = self
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("Profile no longer exists.")?;
        profile.category = (!value.is_empty()).then(|| value.to_owned());
        Ok(())
    }
    fn normalize_metadata(&mut self) {
        for profile in &mut self.profiles {
            if profile.email.is_none() {
                profile.email = profile
                    .login
                    .as_ref()
                    .and_then(|login| login_email(profile.identity.provider, login));
                if profile.category.is_none()
                    && profile.email.as_deref() != Some(&profile.label)
                    && ![
                        "Claude account",
                        "Codex account",
                        "Email unavailable",
                        profile.identity.user_id.as_str(),
                    ]
                    .contains(&profile.label.as_str())
                {
                    profile.category = Some(profile.label.clone());
                }
            }
            profile.label = profile
                .email
                .clone()
                .unwrap_or_else(|| "Email unavailable".into());
        }
    }

    pub fn recovery(&self) -> Option<&Recovery> {
        self.recovery.as_ref()
    }
    pub fn recovery_target(&self) -> Option<&Profile> {
        let journal = self.recovery.as_ref()?;
        self.profiles.iter().find(|p| p.id == journal.target_id)
    }
    pub fn recovery_mut(&mut self) -> Option<&mut Recovery> {
        self.recovery.as_mut()
    }
    pub fn begin_recovery(&mut self, journal: Recovery) {
        self.recovery = Some(journal);
    }
    pub fn end_recovery(&mut self) -> Option<Recovery> {
        self.recovery.take()
    }

    pub fn ticket(&self, guard: Guard<'_>) -> Result<Ticket, String> {
        match guard {
            Guard::SignIn => {
                if self.recovery.is_some() {
                    return Err(RECOVER_FIRST.into());
                }
                Ok(Ticket(Guarded::Epoch(self.login_epoch)))
            }
            Guard::Renewal(profile) => {
                let login = profile
                    .login
                    .as_ref()
                    .ok_or("Sign in again to refresh this account.")?;
                let fingerprint = super::view(profile.identity.provider, login)?.fingerprint();
                let ticket = Ticket(Guarded::Renewal(HeldLogin::of(profile, fingerprint)));
                self.check(&ticket)
                    .map_err(|_| "The saved login changed before renewal.")?;
                Ok(ticket)
            }
        }
    }
    fn check(&self, ticket: &Ticket) -> Result<(), String> {
        let recovering = self.recovery.is_some();
        match &ticket.0 {
            Guarded::Epoch(epoch) if recovering || *epoch != self.login_epoch => {
                Err(SIGN_IN_CHANGED.into())
            }
            Guarded::Reading { epoch, held }
                if recovering || *epoch != self.login_epoch || held.in_vault(self).is_none() =>
            {
                Err("The account state changed while usage was read.".into())
            }
            Guarded::HomeReading { epoch, held }
                if recovering || *epoch != self.login_epoch || held.in_vault(self).is_none() =>
            {
                Err("The account state changed while usage was read.".into())
            }
            Guarded::Renewal(_) if recovering => Err("An account change needs recovery.".into()),
            Guarded::Renewal(held)
                if !held.in_vault(self).is_some_and(|p| p.usage_renewal_owned) =>
            {
                Err("The saved login changed during renewal.".into())
            }
            Guarded::Epoch(_)
            | Guarded::Reading { .. }
            | Guarded::HomeReading { .. }
            | Guarded::Renewal(_) => Ok(()),
        }
    }
    fn admit(&mut self, kind: &ChangeKind<'_>) -> Result<(), String> {
        kind.gate(self)?;
        if kind.bumps() {
            self.invalidate_logins()?;
        }
        Ok(())
    }
    fn invalidate_logins(&mut self) -> Result<(), String> {
        self.login_epoch = self
            .login_epoch
            .checked_add(1)
            .ok_or("Sign-in generation exhausted.")?;
        Ok(())
    }
    pub fn save(
        &mut self,
        identity: Identity,
        login: Login,
        expected: Option<&str>,
    ) -> Result<String, String> {
        if let Some(id) = expected {
            let profile = self
                .profiles
                .iter()
                .find(|p| p.id == id)
                .ok_or("The profile was removed while sign-in was running.")?;
            if profile.identity != identity {
                return Err("Sign-in returned a different user or workspace. The saved profile has not changed.".into());
            }
        }
        let email = login_email(identity.provider, &login);
        if let Some(profile) = self.profiles.iter_mut().find(|p| p.identity == identity) {
            if email.is_some() {
                profile.email = email;
            }
            profile.label = profile
                .email
                .clone()
                .unwrap_or_else(|| "Email unavailable".into());
            profile.pending_activation = false;
            profile.usage_renewal_owned = false;
            profile.login = Some(login);
            profile.saved_at = chrono::Utc::now().to_rfc3339();
            return Ok(profile.id.clone());
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.profiles.push(Profile {
            id: id.clone(),
            identity,
            label: email.clone().unwrap_or_else(|| "Email unavailable".into()),
            email,
            category: None,
            saved_at: chrono::Utc::now().to_rfc3339(),
            login: Some(login),
            pending_activation: false,
            usage_renewal_owned: false,
            home: None,
            signed_out: false,
        });
        Ok(id)
    }
}

const LEASE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn lease_timeout() -> std::time::Duration {
    #[cfg(test)]
    if let Some(timeout) = lease_timeout_override::current() {
        return timeout;
    }
    LEASE_TIMEOUT
}

#[cfg(test)]
pub(crate) mod lease_timeout_override {
    use std::{cell::Cell, time::Duration};

    thread_local! {
        static OVERRIDE: Cell<Option<Duration>> = const { Cell::new(None) };
    }

    pub(super) fn current() -> Option<Duration> {
        OVERRIDE.get()
    }

    #[must_use]
    pub(crate) struct Guard(Option<Duration>);

    pub(crate) fn set(timeout: Duration) -> Guard {
        Guard(OVERRIDE.replace(Some(timeout)))
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OVERRIDE.set(self.0);
        }
    }
}

pub struct Store {
    pub root: std::path::PathBuf,
    key: [u8; 32],
    _lease: FileLease,
}

#[derive(Clone)]
pub struct Sealer([u8; 32]);
impl Sealer {
    pub fn seal(&self, plain: &[u8]) -> Result<Vec<u8>, String> {
        super::vault::seal(&self.0, plain)
    }
    pub fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>, String> {
        super::vault::unseal(&self.0, sealed)
    }
}

pub type Persist<'a> = dyn FnMut(&Database) -> Result<(), String> + 'a;
impl Store {
    pub fn lease(home: &std::path::Path) -> Result<(std::path::PathBuf, FileLease), String> {
        Self::lease_with_timeout(home, lease_timeout())
    }
    fn lease_with_timeout(
        home: &std::path::Path,
        timeout: std::time::Duration,
    ) -> Result<(std::path::PathBuf, FileLease), String> {
        use std::fs::{self, OpenOptions, TryLockError};
        use std::time::{Duration, Instant};
        let root = home.join(".on-n-off/accounts");
        fs::create_dir_all(&root).map_err(|_| "Cannot create profile storage.")?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("operation.lock"))
            .map_err(|_| "Cannot coordinate account changes.")?;
        let started = Instant::now();
        let lease = FileLease::acquire(file, |file| loop {
            match file.try_lock() {
                Ok(()) => return Ok(()),
                Err(TryLockError::WouldBlock) if started.elapsed() < timeout => {
                    std::thread::sleep(
                        Duration::from_millis(20).min(timeout.saturating_sub(started.elapsed())),
                    );
                }
                Err(TryLockError::WouldBlock) => {
                    return Err("Another account operation is running. Retry when it finishes.")
                }
                Err(TryLockError::Error(_)) => return Err("Cannot coordinate account changes."),
            }
        })?;
        Ok((root, lease))
    }
    pub fn vault_exists(home: &std::path::Path) -> bool {
        home.join(".on-n-off/accounts/vault.enc").exists()
    }
    pub fn open(home: &std::path::Path, create: bool) -> Result<Self, String> {
        Self::open_with_key(home, create, |root, create| {
            super::vault::key(root, create, true)
        })
    }
    pub fn open_existing(home: &std::path::Path) -> Result<Self, String> {
        Self::open_with_key(home, false, |root, create| {
            super::vault::key(root, create, false)
        })
    }
    pub(super) fn open_with_key(
        home: &std::path::Path,
        create: bool,
        unlock: impl FnOnce(&std::path::Path, bool) -> Result<[u8; 32], String>,
    ) -> Result<Self, String> {
        let root = home.join(".on-n-off/accounts");
        std::fs::create_dir_all(&root).map_err(|_| "Cannot create profile storage.")?;
        let (key, lease) = if create && !root.join("vault.enc").exists() {
            let (_, lease) = Self::lease(home)?;
            (unlock(&root, !root.join("vault.enc").exists())?, lease)
        } else {
            let key = unlock(&root, false)?;
            let (_, lease) = Self::lease(home)?;
            (key, lease)
        };
        Ok(Self {
            root,
            key,
            _lease: lease,
        })
    }
    pub fn load(&self) -> Result<Database, String> {
        match std::fs::read(self.root.join("vault.enc")) {
            Ok(bytes) => {
                let plain = super::vault::unseal(&self.key, &bytes)?;
                let mut db: Database = serde_json::from_slice(&plain)
                    .map_err(|_| "Invalid saved profile data. No credentials were changed.")?;
                db.normalize_metadata();
                Ok(db)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Database::default()),
            Err(_) => Err("Cannot read saved profile storage.".into()),
        }
    }
    fn persist(&self, db: &Database) -> Result<(), String> {
        self.write(&encode(db)?)
    }
    fn write(&self, plain: &[u8]) -> Result<(), String> {
        let sealed = super::vault::seal(&self.key, plain)?;
        super::vault::atomic_write(&self.root.join("vault.enc"), &sealed)
    }
    pub fn sealer(&self) -> Sealer {
        Sealer(self.key)
    }

    pub fn gate(home: &std::path::Path, kind: &ChangeKind<'_>) -> Result<(), String> {
        if !Self::vault_exists(home) {
            return kind.gate(&Database::default());
        }
        kind.gate(&Self::open(home, false)?.load()?)
    }

    pub fn change<T>(
        self,
        kind: ChangeKind<'_>,
        edit: impl FnOnce(&mut Database) -> Result<T, String>,
    ) -> Result<T, String> {
        self.change_then(kind, edit, |value, _, _| Ok(value))?
    }
    pub fn change_then<E, T>(
        self,
        kind: ChangeKind<'_>,
        edit: impl FnOnce(&mut Database) -> Result<E, String>,
        then: impl FnOnce(E, &mut Database, &mut Persist<'_>) -> Result<T, String>,
    ) -> Result<Result<T, String>, String> {
        self.commit(|db| db.admit(&kind), edit, then)
    }
    pub fn publish<T>(
        self,
        ticket: &Ticket,
        edit: impl FnOnce(&mut Database) -> Result<T, String>,
    ) -> Result<T, String> {
        self.publish_then(ticket, edit, |value, _, _| Ok(value))?
    }
    pub fn publish_then<E, T>(
        self,
        ticket: &Ticket,
        edit: impl FnOnce(&mut Database) -> Result<E, String>,
        then: impl FnOnce(E, &mut Database, &mut Persist<'_>) -> Result<T, String>,
    ) -> Result<Result<T, String>, String> {
        self.commit(|db| db.check(ticket), edit, then)
    }
    pub fn recheck(&self, ticket: &Ticket) -> Result<(), String> {
        self.load()?.check(ticket)
    }
    fn commit<E, T>(
        self,
        admit: impl FnOnce(&mut Database) -> Result<(), String>,
        edit: impl FnOnce(&mut Database) -> Result<E, String>,
        then: impl FnOnce(E, &mut Database, &mut Persist<'_>) -> Result<T, String>,
    ) -> Result<Result<T, String>, String> {
        let mut db = self.load()?;
        let loaded = encode(&db)?;
        admit(&mut db)?;
        let value = edit(&mut db)?;
        let edited = encode(&db)?;
        if edited != loaded {
            self.write(&edited)?;
        }
        let result = then(value, &mut db, &mut |db| self.persist(db));
        drop(self);
        Ok(result)
    }
}

fn encode(db: &Database) -> Result<Vec<u8>, String> {
    serde_json::to_vec(db).map_err(|_| "Cannot encode saved profiles.".into())
}

#[cfg(test)]
pub(crate) mod tests;
