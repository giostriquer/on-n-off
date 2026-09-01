pub mod model;

#[cfg(any(target_os = "macos", test))]
mod protocol;

#[cfg(target_os = "macos")]
mod config;
#[cfg(target_os = "macos")]
mod displays;
#[cfg(target_os = "macos")]
mod transport;
#[cfg(target_os = "macos")]
mod window;

pub use model::{NotchSettings, NotchSnapshot};
#[cfg(target_os = "macos")]
pub use window::{add_runtime_error, setup, shutdown};

pub fn read() -> NotchSnapshot {
    #[cfg(target_os = "macos")]
    {
        let revision = config::revision();
        let settings = config::read();
        let displays = displays::read();
        let error = settings.as_ref().err().or(displays.as_ref().err()).cloned();
        NotchSnapshot {
            revision,
            supported: true,
            settings: settings.unwrap_or_default(),
            displays: displays.unwrap_or_default(),
            error,
        }
    }
    #[cfg(not(target_os = "macos"))]
    NotchSnapshot {
        revision: 0,
        supported: false,
        settings: NotchSettings::default(),
        displays: vec![],
        error: None,
    }
}

pub fn save(settings: NotchSettings) -> Result<NotchSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        let result = displays::read();
        let error = result.as_ref().err().cloned();
        let displays = result.unwrap_or_default();
        if settings.enabled && model::layout(&settings, &displays, false).is_none() {
            return Err("Select a connected display with mirroring turned off.".into());
        }
        let revision = config::save(&settings)?;
        Ok(NotchSnapshot {
            revision,
            supported: true,
            settings,
            displays,
            error,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = settings;
        Err("The side notch is available on macOS only.".into())
    }
}

pub fn apply(app: &tauri::AppHandle, snapshot: NotchSnapshot) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        window::sync(app, snapshot)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, snapshot);
        Ok(())
    }
}
