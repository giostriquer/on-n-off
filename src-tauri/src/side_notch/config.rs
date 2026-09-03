use super::model::NotchSettings;
use crate::read_revision::Revision;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
/// Moves on every save, so a snapshot in flight from before the save is discarded rather than
/// drawn: the same mechanism the shared provider reads use, see [`Revision`].
static REVISION: Revision = Revision::new();

pub fn revision() -> u64 {
    REVISION.current()
}

fn path() -> Result<PathBuf, String> {
    crate::paths::user_home()
        .map(|home| home.join(".on-n-off").join("side-notch.json"))
        .map_err(|error| error.message)
}

pub fn read() -> Result<NotchSettings, String> {
    match fs::read_to_string(path()?) {
        Ok(body) => serde_json::from_str(&body)
            .map_err(|error| format!("Cannot read side-notch settings: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(NotchSettings::default()),
        Err(error) => Err(format!("Cannot read side-notch settings: {error}")),
    }
}

pub fn save(settings: &NotchSettings) -> Result<u64, String> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|_| "Notch settings are unavailable.")?;
    save_to(&path()?, settings, &REVISION)
}

fn save_to(path: &Path, settings: &NotchSettings, counter: &Revision) -> Result<u64, String> {
    let body = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    crate::usage::cache_io::atomic_write(path, &body)
        .map_err(|error| format!("Cannot save side-notch settings: {error}"))?;
    Ok(counter.bump())
}

#[cfg(test)]
mod tests;
