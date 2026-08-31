use super::model::NotchSettings;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
static REVISION: AtomicU64 = AtomicU64::new(0);

pub fn revision() -> u64 {
    REVISION.load(Ordering::SeqCst)
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
    save_at_revision(settings, None)
}

pub fn save_at_revision(settings: &NotchSettings, expected: Option<u64>) -> Result<u64, String> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|_| "Notch settings are unavailable.")?;
    save_to(&path()?, settings, expected, &REVISION)
}

fn save_to(
    path: &Path,
    settings: &NotchSettings,
    expected: Option<u64>,
    counter: &AtomicU64,
) -> Result<u64, String> {
    if expected.is_some_and(|value| value != counter.load(Ordering::SeqCst)) {
        return Err("Settings changed in another window. Try again.".into());
    }
    let body = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    crate::usage::cache_io::atomic_write(path, &body)
        .map_err(|error| format!("Cannot save side-notch settings: {error}"))?;
    Ok(counter.fetch_add(1, Ordering::SeqCst) + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_native_settings_cannot_overwrite_a_main_window_save() {
        let dir = crate::paths::scratch_dir("notch-settings");
        let path = dir.join("side-notch.json");
        let counter = AtomicU64::new(0);
        let current = NotchSettings {
            enabled: true,
            display_id: Some("display-two".into()),
            edge: super::super::model::Edge::Left,
        };
        assert_eq!(save_to(&path, &current, None, &counter).unwrap(), 1);
        assert!(save_to(&path, &NotchSettings::default(), Some(0), &counter).is_err());
        let persisted: NotchSettings =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(persisted, current);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
