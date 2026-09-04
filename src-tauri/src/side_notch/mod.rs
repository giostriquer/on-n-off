pub mod model;

#[cfg(any(target_os = "macos", test))]
mod protocol;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
mod sessions;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod config;
#[cfg(target_os = "macos")]
mod displays;
#[cfg(target_os = "macos")]
mod transport;
#[cfg(target_os = "macos")]
mod window;

#[cfg(target_os = "windows")]
mod win_displays;
#[cfg(target_os = "windows")]
mod win_host;
#[cfg(target_os = "windows")]
mod win_paint;
#[cfg(target_os = "windows")]
mod win_window;

pub use model::{NotchSettings, NotchSnapshot};
#[cfg(target_os = "windows")]
pub use win_host::{add_runtime_error, setup, shutdown};
#[cfg(target_os = "macos")]
pub use window::{add_runtime_error, setup, shutdown};

pub fn read() -> NotchSnapshot {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let revision = config::revision();
        let settings = config::read();
        let displays = displays_result();
        let error = settings.as_ref().err().or(displays.as_ref().err()).cloned();
        NotchSnapshot {
            revision,
            supported: supported(),
            settings: settings.unwrap_or_default(),
            displays: displays.unwrap_or_default(),
            error,
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    NotchSnapshot {
        revision: 0,
        supported: false,
        settings: NotchSettings::default(),
        displays: vec![],
        error: None,
    }
}

/// Displays for the snapshot; macOS shells out to its helper, Windows enumerates here.
#[cfg(target_os = "macos")]
fn displays_result() -> Result<Vec<model::Display>, String> {
    displays::read()
}

#[cfg(target_os = "windows")]
fn displays_result() -> Result<Vec<model::Display>, String> {
    win_displays::read()
}

/// The platform gate: macOS always; Windows only from Windows 11 (build 22000).
fn supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(target_os = "windows")]
    {
        win11()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

/// Windows 11 is build 22000 and up; the notch's edge-docked overlay assumes the
/// Win11 shell. Read from the registry the way `winver` reports it.
#[cfg(target_os = "windows")]
fn win11() -> bool {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let build = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .and_then(|key| key.get_value::<String, _>("CurrentBuildNumber"))
        .ok()
        .and_then(|build| build.trim().parse::<u32>().ok());
    build.is_some_and(|build| build >= 22000)
}

pub fn save(settings: NotchSettings) -> Result<NotchSnapshot, String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let mut settings = settings;
        settings.providers = settings.rail_providers();
        settings.pull_requests.lists = settings.pull_requests.selected_lists();
        let result = displays_result();
        let error = result.as_ref().err().cloned();
        let displays = result.unwrap_or_default();
        if settings.enabled {
            if !supported() {
                return Err("The side notch is available on Windows 11 or later.".into());
            }
            if settings.providers.is_empty() {
                return Err("Choose at least one provider for the notch.".into());
            }
            if model::layout(&settings, &displays).is_none() {
                return Err(
                    "Select a connected display with mirroring turned off and room for the notch."
                        .into(),
                );
            }
        }
        let revision = config::save(&settings)?;
        Ok(NotchSnapshot {
            revision,
            supported: supported(),
            settings,
            displays,
            error,
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
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
    #[cfg(target_os = "windows")]
    {
        win_host::sync(app, snapshot)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (app, snapshot);
        Ok(())
    }
}
