//! Codex's native store: which backend its `config.toml` selects for the signed-in login
//! ([`target`]: `auth.json`, the item Codex files in the OS credential store, or `auto` between
//! them), how that item is named and reached, and the one read of it Limits may make
//! ([`metadata`], [`metadata_and_access`]).
//!
//! It mirrors `claude_store`: every on-n-off path that reads or writes Codex's login finds it here
//! — the account switch, and the Limits and subscription reads through the projections — so they
//! cannot disagree about which store holds it. Codex takes no lock around its login, so there is
//! none to take here.
//!
//! On macOS the credential store is the Keychain, reached through `/usr/bin/security`
//! (`super::keychain`), never this process's own identity; elsewhere it is the `keyring` crate.

use super::{codex::CodexLogin, model, store::Login, vault};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Where a Codex login lives.
enum Target {
    /// `<codex home>/auth.json`.
    File(PathBuf),
    /// The item Codex files for one home in the OS credential store.
    Keyring { service: String, account: String },
}

impl Target {
    /// The stored document; `None` when nothing is stored there.
    fn read(&self) -> Result<Option<Value>, String> {
        let bytes = match self {
            Self::File(path) => match fs::read(path) {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err("Cannot read native credentials.".into()),
            },
            Self::Keyring { service, account } => {
                #[cfg(target_os = "macos")]
                {
                    match super::keychain::find_password(service, Some(account))? {
                        Some(value) => value.into_bytes(),
                        None => return Ok(None),
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    match keyring::Entry::new(service, account)
                        .map_err(|_| "Cannot open native credential store.")?
                        .get_secret()
                    {
                        Ok(v) => v,
                        Err(keyring::Error::NoEntry) => return Ok(None),
                        Err(_) => {
                            return Err("Native credential access was denied or unavailable.".into())
                        }
                    }
                }
            }
        };
        serde_json::from_slice(&bytes).map(Some).map_err(|_| {
            "The native credential document is malformed. It has not been changed.".into()
        })
    }

    /// Replaces the stored document with `value` verbatim, or removes it for `None`.
    fn write(&self, value: Option<&Value>) -> Result<(), String> {
        let bytes = value
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| "Cannot encode native credentials.")?;
        match self {
            Self::File(path) => {
                if let Some(bytes) = bytes {
                    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
                        return Err("Refusing to replace a linked credential file.".into());
                    }
                    vault::atomic_write(path, &bytes)
                } else {
                    match fs::remove_file(path) {
                        Ok(()) => Ok(()),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(_) => Err("Cannot remove native credentials.".into()),
                    }
                }
            }
            Self::Keyring { service, account } => {
                // Through `security`, the identity the item already trusts for reads, never this
                // ad-hoc-signed process: `keychain.rs` says what the latter cost.
                #[cfg(target_os = "macos")]
                {
                    match bytes {
                        Some(bytes) => super::keychain::write(service, account, &bytes),
                        None => super::keychain::delete(service, account),
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let entry = keyring::Entry::new(service, account)
                        .map_err(|_| "Cannot open native credential store.")?;
                    if let Some(bytes) = bytes {
                        entry
                            .set_secret(&bytes)
                            .map_err(|_| "Cannot update native credential store.".into())
                    } else {
                        match entry.delete_credential() {
                            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                            Err(_) => Err("Cannot remove native credentials.".into()),
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn config_file(config_home: &Path) -> PathBuf {
    config_home.join("config.toml")
}

/// Codex's configuration for the home `config_home`; an empty table when there is none.
pub(super) fn config(config_home: &Path) -> Result<toml::Value, String> {
    match fs::read_to_string(config_file(config_home)) {
        Ok(text) => toml::from_str(&text).map_err(|_| "Native configuration is malformed.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(toml::Value::Table(Default::default()))
        }
        Err(_) => Err("Cannot read native configuration.".into()),
    }
}

/// Where the login for `config_home` lives, from the backend its config selects:
/// `cli_auth_credentials_store` is `file` (the default), `keyring`, or `auto`, which uses the
/// credential-store item when there is one and the file otherwise. Any other backend is refused,
/// and so is a config that names a `profile`, whose effective backend cannot be verified.
fn target(config_home: &Path) -> Result<Target, String> {
    let config = config(config_home)?;
    if config.get("profile").is_some() {
        return Err("Codex configuration profiles must use the official account controls until their effective credential backend can be verified.".into());
    }
    let mode = config
        .get("cli_auth_credentials_store")
        .and_then(toml::Value::as_str)
        .unwrap_or("file");
    let file = Target::File(config_home.join("auth.json"));
    match mode {
        "file" => Ok(file),
        "keyring" | "auto" => {
            let canonical =
                fs::canonicalize(config_home).map_err(|_| "Cannot resolve native Codex home.")?;
            let hash = crate::sha::sha256_hex(canonical.to_string_lossy().as_bytes());
            let keyring = Target::Keyring {
                service: "Codex Auth".into(),
                account: format!("cli|{}", &hash[..16]),
            };
            if mode == "auto" && keyring.read()?.is_none() {
                Ok(file)
            } else {
                Ok(keyring)
            }
        }
        _ => Err(
            "This Codex credential backend cannot be activated by on-n-off. Use official sign-in."
                .into(),
        ),
    }
}

/// The login stored for `config_home`, wherever its config puts it. Codex keeps no account record
/// beside it.
pub(super) fn read(config_home: &Path) -> Result<Option<Login>, String> {
    Ok(target(config_home)?.read()?.map(|auth| Login {
        auth,
        account: Value::Null,
    }))
}

/// Replaces the login stored for `config_home`, wherever its config puts it, with `auth` verbatim,
/// or removes it for `None`.
pub(super) fn write(config_home: &Path, auth: Option<&Value>) -> Result<(), String> {
    target(config_home)?.write(auth)
}

/// The key a Codex card is known by: the user and workspace together when the claims name the
/// user, the bare workspace otherwise, as older observations were keyed.
pub(crate) fn observation_key(workspace: &str, claims: &Value) -> String {
    claims
        .get("chatgpt_user_id")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .map(|user| {
            model::Identity {
                provider: crate::dto::AgentId::Codex,
                user_id: user.into(),
                workspace_id: workspace.into(),
            }
            .observation_key()
        })
        .unwrap_or_else(|| workspace.into())
}

/// Metadata projection from the backend selected by native Codex config. No credential leaves
/// accounts here; `metadata_and_access` is the one projection that carries the access token.
pub(crate) fn metadata(config_home: &Path) -> Result<Option<Metadata>, String> {
    match read(config_home)? {
        Some(login) => identity(&login),
        None => Ok(None),
    }
}

/// A Codex login's observation key and workspace claims, refusing claims for another workspace.
fn identity(login: &Login) -> Result<Option<Metadata>, String> {
    let login = CodexLogin::of(login);
    let Some(workspace) = login.workspace() else {
        return Ok(None);
    };
    let claims = if login.has_id_token() {
        let claims = login
            .auth_claims()?
            .ok_or("Missing native account claims.")?;
        if claims.get("chatgpt_account_id").and_then(Value::as_str) != Some(workspace) {
            return Err("Native workspace claims disagree.".into());
        }
        claims
    } else {
        json!({"chatgpt_account_id":workspace})
    };
    Ok(Some((observation_key(workspace, &claims), claims)))
}

/// The signed-in Codex login's access token, beside the identity it belongs to, for the two requests
/// on-n-off makes with that login itself: a workspace member's spending (`limits/credits_spent.rs`)
/// and the subscription's term (`limits/renewal.rs`), read-only GETs the user chose to allow on
/// 2026-09-24 and 2026-09-25. Only the access token leaves accounts.
pub(crate) struct CodexAccess {
    /// The same key `metadata` gives, so the caller can match the token to a card.
    pub observation_key: String,
    /// The `ChatGPT-Account-Id` the request is made for.
    pub workspace_id: String,
    pub token: model::AccessToken,
}

/// A metadata projection: the observation key and the workspace claims (`metadata`).
pub(crate) type Metadata = (String, Value);

/// `metadata`, and the login's access projection when it holds an access token, from one read of
/// the native store: the signed-in read's identity check after the app-server handshake takes it,
/// so the backend reads cost no read of their own. `None` for no login; the access is `None` for a
/// login without an access token.
pub(crate) fn metadata_and_access(
    config_home: &Path,
) -> Result<Option<(Metadata, Option<CodexAccess>)>, String> {
    let Some(login) = read(config_home)? else {
        return Ok(None);
    };
    let Some((observation_key, claims)) = identity(&login)? else {
        return Ok(None);
    };
    let access = match CodexLogin::of(&login).access_token() {
        Some(token) => Some(CodexAccess {
            observation_key: observation_key.clone(),
            workspace_id: claims
                .get("chatgpt_account_id")
                .and_then(Value::as_str)
                .ok_or("Missing native account claims.")?
                .to_string(),
            token,
        }),
        None => None,
    };
    Ok(Some(((observation_key, claims), access)))
}

#[cfg(test)]
mod tests;
