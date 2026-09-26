//! The saved-profile vault and the protocol every change to it follows. An account change goes
//! through `Store::change`, which refuses it during a pending recovery, rejects every sign-in in
//! flight by bumping the sign-in epoch, persists it and releases the lease before the caller
//! announces it. Work too slow to hold the lease takes a `Ticket` first, rechecked under the lease
//! afterwards: a sign-in's by `Store::change` with `ChangeKind::SignIn`, which then bumps the epoch
//! as any account change does; a remembered login's and a renewal's by `Store::publish`, which bumps
//! nothing; a usage reading's by `Store::recheck`, since the reading is published outside the vault.
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
    /// True only while an isolated app sign-in has never been published to a native client.
    #[serde(default)]
    pub usage_renewal_owned: bool,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Database {
    pub profiles: Vec<Profile>,
    /// The sign-in epoch: every account change bumps it, and a sign-in, a remembered login or a
    /// usage reading that started before it publishes nothing.
    #[serde(default)]
    login_epoch: u64,
    #[serde(default)]
    pub ignored_accounts: Vec<Identity>,
    #[serde(default)]
    pub ignored_credentials: Vec<String>,
    /// The journal of a switch that was interrupted, which every account change but recovery and
    /// a metadata edit waits for.
    #[serde(default)]
    recovery: Option<Recovery>,
}

/// Refused while an interrupted switch awaits recovery.
const RECOVER_FIRST: &str = "Recover the interrupted account change first.";
/// A sign-in or a remembered login whose ticket no longer holds.
const SIGN_IN_CHANGED: &str = "The account state changed while sign-in was running. Start sign-in again after recovery or account changes finish.";

/// Which change `Store::change` makes to the vault, and so which of its rules it follows: an
/// account change (`Account`, `SignIn`, `Recovery`), remembering turned on, or a metadata edit.
/// `gate` and `bumps` spell out each kind's rule.
pub enum ChangeKind<'t> {
    /// Changes which logins are saved or which one a CLI uses (save, remove, use, sign out):
    /// refused during a pending recovery, and rejects every sign-in in flight.
    Account,
    /// Publishes a sign-in, which is itself an account change: refused unless its ticket still
    /// holds.
    SignIn(&'t Ticket),
    /// Recovers the interrupted switch, so it requires one; rejects every sign-in in flight.
    Recovery,
    /// Turns automatic remembering on. It rejects every sign-in or remembered login in flight, so
    /// a check made before the person opted out cannot publish after they opt back in. It changes
    /// no login, so a pending recovery does not refuse it.
    Remembering,
    /// Edits a profile's display metadata: allowed during recovery, and rejects nothing in flight.
    Metadata,
}

impl ChangeKind<'_> {
    /// Refuses this change when its rule does, before anything is written.
    fn gate(&self, db: &Database) -> Result<(), String> {
        match self {
            Self::Account if db.recovery.is_some() => Err(RECOVER_FIRST.into()),
            Self::Recovery if db.recovery.is_none() => Err("No recovery is pending.".into()),
            Self::SignIn(ticket) => db.check(ticket),
            Self::Account | Self::Recovery | Self::Remembering | Self::Metadata => Ok(()),
        }
    }
    /// Whether this change rejects every sign-in, remembered login and usage reading in flight.
    fn bumps(&self) -> bool {
        match self {
            Self::Account | Self::SignIn(_) | Self::Recovery | Self::Remembering => true,
            Self::Metadata => false,
        }
    }
}

/// What a publication after slow work, which ran without the vault lease, still requires of the
/// vault. Taken with `Database::ticket` before the work and rechecked under the lease after it by
/// `ChangeKind::SignIn` (a sign-in), `Store::publish` (a remembered login, a renewal) or
/// `Store::recheck` (a usage reading). A pending recovery rejects every ticket.
#[derive(Clone)]
pub struct Ticket(Guarded);
#[derive(Clone)]
enum Guarded {
    /// The sign-in epoch the ticket was taken at: any account change since rejects it.
    Epoch(u64),
    /// The epoch, and a saved profile still holding the login a usage reading was made with.
    Reading { epoch: u64, held: HeldLogin },
    /// A saved profile still holding the login that was renewed, and still owning its renewal.
    /// There is no epoch to compare: the renewal has spent the refresh token by the time it
    /// publishes, so an account change that left this profile alone must not reject it.
    Renewal(HeldLogin),
}
#[derive(Clone)]
struct HeldLogin {
    id: String,
    identity: Identity,
    fingerprint: String,
}
impl HeldLogin {
    fn of(profile: &Profile, login: &Login) -> Self {
        Self {
            id: profile.id.clone(),
            identity: profile.identity.clone(),
            fingerprint: login.fingerprint(),
        }
    }
    /// The saved profile that still holds this login, if one does.
    fn in_vault<'db>(&self, db: &'db Database) -> Option<&'db Profile> {
        db.profiles.iter().find(|profile| {
            profile.id == self.id
                && profile.identity == self.identity
                && profile.login.as_ref().map(Login::fingerprint).as_ref()
                    == Some(&self.fingerprint)
        })
    }
}

/// What a ticket guards.
pub enum Guard<'a> {
    /// The sign-in epoch: any account change since the ticket rejects the publication. A sign-in,
    /// a remembered login and a usage reading take this.
    SignIn,
    /// A private renewal of this profile: it must still hold the login that was renewed and still
    /// own its renewal. Deliberately not the epoch: the renewal has spent the refresh token by the
    /// time it publishes, so an account change that left this profile alone must not reject the
    /// renewed login, or it would be lost.
    Renewal(&'a Profile),
}

impl Ticket {
    /// This ticket, now also rejected once `profile` no longer holds `login`: the generation a
    /// usage reading was made with. A renewal's ticket moves to that login, still without an epoch.
    pub fn holding(&self, profile: &Profile, login: &Login) -> Self {
        let held = HeldLogin::of(profile, login);
        Self(match self.0 {
            Guarded::Epoch(epoch) | Guarded::Reading { epoch, .. } => {
                Guarded::Reading { epoch, held }
            }
            Guarded::Renewal(_) => Guarded::Renewal(held),
        })
    }
}
impl Login {
    pub fn fingerprint(&self) -> String {
        // Presentation metadata and refreshed identity claims do not create a new OAuth generation.
        let generation = (
            self.auth.pointer("/claudeAiOauth/accessToken"),
            self.auth.pointer("/claudeAiOauth/refreshToken"),
            self.auth.pointer("/tokens/access_token"),
            self.auth.pointer("/tokens/refresh_token"),
        );
        crate::sha::sha256_hex(
            &serde_json::to_vec(&generation).expect("serializable credential generation"),
        )
    }
    pub fn email(&self, provider: crate::dto::AgentId) -> Option<String> {
        let email = if provider == crate::dto::AgentId::Claude {
            self.account
                .get("emailAddress")
                .and_then(Value::as_str)
                .map(str::to_owned)
        } else {
            super::model::claims(&self.auth).ok().and_then(|claims| {
                claims
                    .get("email")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
        };
        email.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
    }
}
impl Database {
    /// The ID-token claims of the saved Codex profile with this observation key, when it has a login.
    pub fn codex_claims(&self, key: &str) -> Option<Value> {
        let profile = self.profiles.iter().find(|p| {
            p.identity.provider == crate::dto::AgentId::Codex && p.identity.observation_key() == key
        })?;
        super::model::claims(&profile.login.as_ref()?.auth)
            .ok()?
            .get("https://api.openai.com/auth")
            .cloned()
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
                    .and_then(|login| login.email(profile.identity.provider));
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

    /// The interrupted switch awaiting recovery, if any.
    pub fn recovery(&self) -> Option<&Recovery> {
        self.recovery.as_ref()
    }
    /// The saved profile an interrupted switch was switching to, while it awaits recovery.
    pub fn recovery_target(&self) -> Option<&Profile> {
        let journal = self.recovery.as_ref()?;
        self.profiles.iter().find(|p| p.id == journal.target_id)
    }
    /// The pending journal, for the switch that wrote it to update.
    pub fn recovery_mut(&mut self) -> Option<&mut Recovery> {
        self.recovery.as_mut()
    }
    /// Journals a switch before any native write, so a crash leaves it to recover.
    pub fn begin_recovery(&mut self, journal: Recovery) {
        self.recovery = Some(journal);
    }
    /// Clears the journal once the switch finished or was undone, giving it back.
    pub fn end_recovery(&mut self) -> Option<Recovery> {
        self.recovery.take()
    }

    /// A ticket for a publication after slow work that runs without the vault lease. Refused
    /// during a pending recovery, and for a renewal whose profile no longer holds its login.
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
                let ticket = Ticket(Guarded::Renewal(HeldLogin::of(profile, login)));
                self.check(&ticket)
                    .map_err(|_| "The saved login changed before renewal.")?;
                Ok(ticket)
            }
        }
    }
    /// Whether this vault still vouches for `ticket`.
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
            Guarded::Renewal(_) if recovering => Err("An account change needs recovery.".into()),
            Guarded::Renewal(held)
                if !held.in_vault(self).is_some_and(|p| p.usage_renewal_owned) =>
            {
                Err("The saved login changed during renewal.".into())
            }
            Guarded::Epoch(_) | Guarded::Reading { .. } | Guarded::Renewal(_) => Ok(()),
        }
    }
    /// Refuses `kind` by its rule, then bumps the sign-in epoch if the kind does.
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
        let email = login.email(identity.provider);
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
        });
        Ok(id)
    }
}

/// How long an account operation waits for another one to finish before reporting it running.
const LEASE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn lease_timeout() -> std::time::Duration {
    #[cfg(test)]
    if let Some(timeout) = lease_timeout_override::current() {
        return timeout;
    }
    LEASE_TIMEOUT
}

/// Shortens `Store::lease`'s wait on the current thread until the guard drops, so a test of a
/// busy vault does not sit out the production timeout.
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

/// Seals records kept beside the vault, such as a private renewal's journal, with the vault's key,
/// and keeps them readable once the lease is released, without handing the key itself out.
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

/// What `persist` does for a follow-up that must write the vault again under the same lease.
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
                    // Blocking workers serialize reads and writes; brief contention is not an error.
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
    /// Whether this home has a saved-profile vault at all, so a read for a device that never saved
    /// an account neither unlocks nor creates one.
    pub fn vault_exists(home: &std::path::Path) -> bool {
        home.join(".on-n-off/accounts/vault.enc").exists()
    }
    pub fn open(home: &std::path::Path, create: bool) -> Result<Self, String> {
        Self::open_with_key(home, create, |root, create| {
            super::vault::key(root, create, true)
        })
    }
    /// Opens an existing vault for a background read or publication, without asking the OS again
    /// for a key it refused. It never creates a vault, but its lease is as exclusive as any other.
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
            // First creation remains serialized across processes. Recheck under the lease so
            // an existing encrypted vault can never receive a replacement key.
            let (_, lease) = Self::lease(home)?;
            (unlock(&root, !root.join("vault.enc").exists())?, lease)
        } else {
            // Unlock existing storage before taking its file lease: a Keychain prompt is not
            // an account transaction and must not cause cross-provider lock timeouts.
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

    /// Refuses `kind` now if its rule would, for an operation about to do native work that a
    /// refused change must never do: verifying a login, which for Claude may renew and rewrite it.
    /// It creates no vault and writes nothing; `change` gates again under its own lease. An
    /// explicit action may retry a vault unlock the OS refused, so this is not `open_existing`.
    pub fn gate(home: &std::path::Path, kind: &ChangeKind<'_>) -> Result<(), String> {
        if !Self::vault_exists(home) {
            return kind.gate(&Database::default());
        }
        kind.gate(&Self::open(home, false)?.load()?)
    }

    /// Makes one change under this store's lease: refuses it by `kind`'s rule, bumps the
    /// sign-in epoch unless it is a metadata edit, lets `edit` change the database, persists it if
    /// anything changed and releases the lease before returning, so the caller announces the
    /// change after release.
    pub fn change<T>(
        self,
        kind: ChangeKind<'_>,
        edit: impl FnOnce(&mut Database) -> Result<T, String>,
    ) -> Result<T, String> {
        self.change_then(kind, edit, |value, _, _| Ok(value))?
    }
    /// `change`, then `then` with what `edit` returned, still under the lease once the change is
    /// durable: the native half of a use or a sign-out, which may persist again. The outer error
    /// means the change was refused or not written; the inner result is `then`'s.
    pub fn change_then<E, T>(
        self,
        kind: ChangeKind<'_>,
        edit: impl FnOnce(&mut Database) -> Result<E, String>,
        then: impl FnOnce(E, &mut Database, &mut Persist<'_>) -> Result<T, String>,
    ) -> Result<Result<T, String>, String> {
        self.commit(|db| db.admit(&kind), edit, then)
    }
    /// Publishes after slow work that ran without the lease: loads the vault under this lease,
    /// refuses unless it still vouches for `ticket`, lets `edit` change it, persists it if `edit`
    /// changed anything and releases the lease. A publication is not an account change, so it
    /// bumps nothing, and one that finds nothing left to publish writes nothing.
    pub fn publish<T>(
        self,
        ticket: &Ticket,
        edit: impl FnOnce(&mut Database) -> Result<T, String>,
    ) -> Result<T, String> {
        self.publish_then(ticket, edit, |value, _, _| Ok(value))?
    }
    /// `publish`, then `then` with what `edit` returned, still under the lease once the
    /// publication is durable.
    pub fn publish_then<E, T>(
        self,
        ticket: &Ticket,
        edit: impl FnOnce(&mut Database) -> Result<E, String>,
        then: impl FnOnce(E, &mut Database, &mut Persist<'_>) -> Result<T, String>,
    ) -> Result<Result<T, String>, String> {
        self.commit(|db| db.check(ticket), edit, then)
    }
    /// Whether the vault, loaded under this store's lease, still vouches for `ticket`: for a
    /// publication outside the vault, a usage reading, that the caller makes while it still holds
    /// the lease.
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
