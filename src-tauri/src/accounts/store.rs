use super::model::Identity;
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
    #[serde(default)]
    pub login_epoch: u64,
    #[serde(default)]
    pub ignored_accounts: Vec<Identity>,
    #[serde(default)]
    pub ignored_credentials: Vec<String>,
    #[serde(default)]
    pub recovery: Option<super::transaction::Recovery>,
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
    pub fn billing_identity(&self, key: &str) -> Option<Identity> {
        self.profiles
            .iter()
            .find(|p| {
                p.identity.provider == crate::dto::AgentId::Codex
                    && p.identity.observation_key() == key
            })
            .map(|p| p.identity.clone())
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

    pub fn invalidate_logins(&mut self) -> Result<(), String> {
        self.login_epoch = self
            .login_epoch
            .checked_add(1)
            .ok_or("Sign-in generation exhausted.")?;
        Ok(())
    }
    pub fn allow_publication(&self, epoch: u64) -> Result<(), String> {
        if self.recovery.is_some() || self.login_epoch != epoch {
            return Err("The account state changed while sign-in was running. Start sign-in again after recovery or account changes finish.".into());
        }
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

pub struct Store {
    pub root: std::path::PathBuf,
    pub(super) key: [u8; 32],
    _lease: FileLease,
}
impl Store {
    pub fn lease(home: &std::path::Path) -> Result<(std::path::PathBuf, FileLease), String> {
        Self::lease_with_timeout(home, std::time::Duration::from_secs(10))
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
    pub fn open(home: &std::path::Path, create: bool) -> Result<Self, String> {
        Self::open_with_key(home, create, |root, create| {
            super::vault::key(root, create, true)
        })
    }
    pub fn open_read(home: &std::path::Path) -> Result<Self, String> {
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
    pub fn with_billing_identity<T>(
        self,
        key: &str,
        consume: impl FnOnce(Result<Option<Identity>, String>) -> T,
    ) -> T {
        let identity = self.load().map(|db| db.billing_identity(key));
        let result = consume(identity);
        drop(self);
        result
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
    pub fn persist(&self, db: &Database) -> Result<(), String> {
        let plain = serde_json::to_vec(db).map_err(|_| "Cannot encode saved profiles.")?;
        let sealed = super::vault::seal(&self.key, &plain)?;
        super::vault::atomic_write(&self.root.join("vault.enc"), &sealed)
    }
}

#[cfg(test)]
mod tests;
