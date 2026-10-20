use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde_json::{json, Map, Value};

use super::model::{AccessToken, Identity};
use crate::dto::AgentId;

#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "ai.meta.dev.credentials";
#[cfg(target_os = "macos")]
const KEYCHAIN_ACCOUNT: &str = "meta";
const SECRETS: [&str; 2] = ["access_token", "api_key"];
const DEFAULT_SCHEMA_VERSION: i64 = 1;
const MALFORMED: &str = "The Muse login document is malformed. It has not been changed.";
const MALFORMED_ITEM: &str = "The Muse Keychain login is malformed. It has not been changed.";
const UNREACHABLE: &str =
    "Muse keeps this login in the Keychain, which on-n-off cannot reach here.";

pub(crate) struct MuseLogin {
    pub(crate) email: Option<String>,
    pub(crate) credential: MuseCredential,
}

pub(crate) enum MuseCredential {
    Token(AccessToken),
    ApiKey,
    Expired,
    Unread,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Storage {
    Inline,
    Keychain,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct NativeLogin {
    pub(super) metadata: Map<String, Value>,
    pub(super) secret: Map<String, Value>,
}

pub(super) trait Keychain {
    fn read(&self) -> Result<Option<String>, String>;
    fn write(&self, raw: &[u8]) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

pub(super) struct SystemKeychain;

impl Keychain for SystemKeychain {
    fn read(&self) -> Result<Option<String>, String> {
        #[cfg(target_os = "macos")]
        return super::keychain::find_password(KEYCHAIN_SERVICE, Some(KEYCHAIN_ACCOUNT));
        #[cfg(not(target_os = "macos"))]
        Err(UNREACHABLE.into())
    }

    fn write(&self, raw: &[u8]) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        return super::keychain::write(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, raw);
        #[cfg(not(target_os = "macos"))]
        {
            let _ = raw;
            Err("This platform has no Keychain.".into())
        }
    }

    fn delete(&self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        return super::keychain::delete(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT);
        #[cfg(not(target_os = "macos"))]
        Ok(())
    }
}

pub(crate) fn identity(email: &str) -> Option<Identity> {
    let email = email.trim();
    email.contains('@').then(|| Identity {
        provider: AgentId::Muse,
        user_id: email.to_lowercase(),
        workspace_id: String::new(),
    })
}

struct Meta {
    metadata: Map<String, Value>,
    inline: Map<String, Value>,
    storage: Option<Storage>,
    expires_at: Option<i64>,
}

impl Meta {
    fn email(&self) -> Option<String> {
        non_empty(self.metadata.get("user_email").and_then(Value::as_str))
    }
}

fn document(text: &str) -> Result<Map<String, Value>, String> {
    match serde_json::from_str(text) {
        Ok(Value::Object(document)) => Ok(document),
        _ => Err(MALFORMED.into()),
    }
}

fn metadata_of(meta: &Map<String, Value>) -> Map<String, Value> {
    meta.iter()
        .filter(|(name, _)| !SECRETS.contains(&name.as_str()) && name.as_str() != "storage")
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

fn meta_of(document: &Map<String, Value>) -> Option<Meta> {
    let meta = document.get("providers")?.get("meta")?.as_object()?;
    let metadata = metadata_of(meta);
    let inline = secret_of(meta);
    let storage = if !inline.is_empty() {
        Some(Storage::Inline)
    } else if meta.get("storage").and_then(Value::as_str) == Some("keychain")
        || meta.get("mechanism").and_then(Value::as_str) == Some("oauth")
    {
        Some(Storage::Keychain)
    } else {
        None
    };
    Some(Meta {
        expires_at: meta.get("expires_at").and_then(Value::as_i64),
        metadata,
        inline,
        storage,
    })
}

fn secret_of(source: &Map<String, Value>) -> Map<String, Value> {
    SECRETS
        .iter()
        .filter_map(|name| {
            let value = non_empty(source.get(*name).and_then(Value::as_str))?;
            Some(((*name).to_string(), Value::String(value)))
        })
        .collect()
}

fn bundle(raw: &str) -> Result<Map<String, Value>, String> {
    match serde_json::from_str(raw.trim()) {
        Ok(Value::Object(bundle)) => Ok(bundle),
        _ => Err(MALFORMED_ITEM.into()),
    }
}

fn read_auth(config: &Path) -> Result<Option<(String, String)>, String> {
    let path = config.join("auth.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot read the Muse login.".into()),
    };
    let modified = std::fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |at| at.as_nanos());
    let stamp = format!("{}:{modified}", crate::sha::sha256_hex(text.as_bytes()));
    Ok(Some((text, stamp)))
}

type Lookup = Result<Option<String>, String>;
type Fields = Map<String, Value>;
type Secret = Result<Option<Fields>, String>;

fn parsed(lookup: Lookup) -> Secret {
    match lookup? {
        Some(raw) => bundle(&raw).map(|bundle| Some(secret_of(&bundle))),
        None => Ok(None),
    }
}

struct Cached {
    config: PathBuf,
    stamp: String,
    secret: Secret,
}

pub(crate) struct LoginCache(Mutex<Option<Cached>>);

impl LoginCache {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(None))
    }

    fn keychain(&self, config: &Path, stamp: &str, lookup: &dyn Fn() -> Lookup) -> Secret {
        let mut cache = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(cached) = cache
            .as_ref()
            .filter(|cached| cached.config == config && cached.stamp == stamp)
        {
            return cached.secret.clone();
        }
        let answer = parsed(lookup());
        *cache = Some(Cached {
            config: config.to_path_buf(),
            stamp: stamp.to_string(),
            secret: answer.clone(),
        });
        answer
    }

    fn forget(&self, config: &Path) {
        let mut cache = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.as_ref().is_some_and(|cached| cached.config == config) {
            *cache = None;
        }
    }

    pub(crate) fn login(
        &self,
        config: &Path,
        now_secs: i64,
        lookup: &dyn Fn() -> Lookup,
    ) -> Option<MuseLogin> {
        let (text, stamp) = read_auth(config).ok()??;
        let meta = meta_of(&document(&text).ok()?)?;
        let storage = meta.storage?;
        let credential = if meta
            .expires_at
            .is_some_and(|expires_at| expires_at <= now_secs)
        {
            MuseCredential::Expired
        } else {
            match storage {
                Storage::Inline => credential_of(&meta.inline)?,
                Storage::Keychain => match self.keychain(config, &stamp, lookup) {
                    Ok(secret) => credential_of(&secret?)?,
                    Err(_) => MuseCredential::Unread,
                },
            }
        };
        Some(MuseLogin {
            email: meta.email(),
            credential,
        })
    }

    pub(super) fn native(
        &self,
        config: &Path,
        keychain: Option<&dyn Keychain>,
    ) -> Result<Option<NativeLogin>, String> {
        let Some((text, stamp)) = read_auth(config)? else {
            return Ok(None);
        };
        let Some(meta) = meta_of(&document(&text)?) else {
            return Ok(None);
        };
        let secret = match meta.storage {
            None => return Ok(None),
            Some(Storage::Inline) => meta.inline,
            Some(Storage::Keychain) => {
                let keychain = keychain.ok_or(UNREACHABLE)?;
                match self.keychain(config, &stamp, &|| keychain.read())? {
                    Some(secret) => secret,
                    None => return Ok(None),
                }
            }
        };
        if secret.is_empty() {
            return Ok(None);
        }
        Ok(Some(NativeLogin {
            metadata: meta.metadata,
            secret,
        }))
    }

    pub(super) fn write(
        &self,
        config: &Path,
        keychain: Option<&dyn Keychain>,
        login: Option<(&Fields, &Fields)>,
    ) -> Result<(), String> {
        let file = config.join("auth.json");
        if std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Refusing to replace a linked Muse login file.".into());
        }
        let mut document = match read_auth(config)? {
            Some((text, _)) => document(&text)?,
            None => Map::new(),
        };
        let target =
            meta_of(&document)
                .and_then(|meta| meta.storage)
                .unwrap_or(if keychain.is_some() {
                    Storage::Keychain
                } else {
                    Storage::Inline
                });
        let reachable = || keychain.ok_or(UNREACHABLE);
        let providers = document
            .entry("providers")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or(MALFORMED)?;
        match login {
            None => {
                providers.remove("meta");
                if target == Storage::Keychain {
                    reachable()?.delete()?;
                }
            }
            Some((metadata, secret)) => {
                let mut meta = metadata_of(metadata);
                let secret = secret_of(secret);
                match target {
                    Storage::Keychain => {
                        let keychain = reachable()?;
                        let version = match keychain.read()? {
                            Some(raw) => bundle(&raw)?.get("secret_schema_version").cloned(),
                            None => None,
                        }
                        .unwrap_or_else(|| json!(DEFAULT_SCHEMA_VERSION));
                        let mut item = secret;
                        item.insert("secret_schema_version".into(), version);
                        keychain.write(
                            &serde_json::to_vec(&item)
                                .map_err(|_| "Cannot encode the Muse login.")?,
                        )?;
                        meta.insert("storage".into(), json!("keychain"));
                    }
                    Storage::Inline => meta.extend(secret),
                }
                providers.insert("meta".into(), Value::Object(meta));
                document.entry("schema_version").or_insert_with(|| json!(1));
            }
        }
        let bytes =
            serde_json::to_vec_pretty(&document).map_err(|_| "Cannot encode the Muse login.")?;
        let written = super::vault::atomic_write(&file, &bytes);
        self.forget(config);
        written
    }
}

fn credential_of(secret: &Fields) -> Option<MuseCredential> {
    match token_of(secret) {
        Some(token) => Some(MuseCredential::Token(AccessToken::new(&token))),
        None => secret
            .contains_key("api_key")
            .then_some(MuseCredential::ApiKey),
    }
}

fn token_of(source: &Map<String, Value>) -> Option<String> {
    non_empty(source.get("access_token").and_then(Value::as_str))
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) static SIGNED_IN: LoginCache = LoginCache::new();

pub(crate) fn signed_in(config: &Path) -> Option<MuseLogin> {
    let now_secs = chrono::Utc::now().timestamp();
    SIGNED_IN.login(config, now_secs, &|| SystemKeychain.read())
}

#[cfg(test)]
mod tests;
