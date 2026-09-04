//! Windows display enumeration for the side notch. One entry per active monitor, with a
//! stable EDID-derived id, in the same point coordinates the macOS reader reports, so the
//! shared `model::layout` math applies unchanged. Read-only.

#![allow(unsafe_code)]

use super::model::Display;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_DEVICE_INFO_TYPE, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORENUMPROC, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
};

struct CachedDisplays {
    topology: String,
    displays: Vec<Display>,
    checked_at: Instant,
}
static CACHE: Mutex<Option<CachedDisplays>> = Mutex::new(None);

/// A monitor as the OS reports it: physical pixels plus its GDI device name.
#[derive(Clone, PartialEq)]
struct RawMonitor {
    device: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    work_y: i32,
    work_height: i32,
    dpi: u32,
}

pub fn read() -> Result<Vec<Display>, String> {
    let _pm_v2 = thread_pm_v2();
    let raw = enum_monitors()?;
    let topology = topology(&raw);
    {
        let cache = CACHE.lock().map_err(|_| "Display cache is unavailable.")?;
        if let Some(cached) = cache.as_ref() {
            if cached.topology == topology && cached.checked_at.elapsed() < Duration::from_secs(60)
            {
                return Ok(cached.displays.clone());
            }
        }
    }
    let (identities, duplicated) = target_identities(&raw);
    let mut displays: Vec<Display> = raw
        .iter()
        .map(|monitor| {
            let (id, name) = identities
                .get(&monitor.device.to_ascii_lowercase())
                .cloned()
                .unwrap_or_else(|| (monitor.device.clone(), monitor.device.clone()));
            to_display(monitor, &id, &name)
        })
        .collect();
    // One GDI source driving two active targets is a duplicate (mirrored) desktop;
    // shared rects are the fallback signal when the topology API did not answer.
    apply_mirroring(&mut displays, duplicated || shared_rects(&raw));
    if displays.iter().any(|display| {
        display.id.is_empty()
            || ![
                display.x,
                display.y,
                display.width,
                display.height,
                display.work_y,
                display.work_height,
                display.scale,
            ]
            .iter()
            .all(|n| n.is_finite())
            || display.scale <= 0.0
    }) {
        return Err("Windows returned incomplete display information.".into());
    }
    let mut cache = CACHE.lock().map_err(|_| "Display cache is unavailable.")?;
    *cache = Some(CachedDisplays {
        topology,
        displays: displays.clone(),
        checked_at: Instant::now(),
    });
    Ok(displays)
}

/// Per-monitor-v2 awareness on the calling thread, restored afterwards, so
/// `GetDpiForMonitor` reports the monitor's own DPI wherever the caller runs.
pub(super) fn thread_pm_v2() -> impl Drop {
    let previous =
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    Restore(previous)
}
struct Restore(DPI_AWARENESS_CONTEXT);
impl Drop for Restore {
    fn drop(&mut self) {
        unsafe { SetThreadDpiAwarenessContext(self.0) };
    }
}

fn enum_monitors() -> Result<Vec<RawMonitor>, String> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        state: LPARAM,
    ) -> windows::core::BOOL {
        let monitors = unsafe { &mut *(state.0 as *mut Vec<RawMonitor>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        let ok = unsafe {
            GetMonitorInfoW(monitor, &mut info as *mut MONITORINFOEXW as *mut _).as_bool()
        };
        if !ok {
            return windows::core::BOOL(0);
        }
        let mut dpi = 96u32;
        let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi) };
        let rect = info.monitorInfo.rcMonitor;
        let work = info.monitorInfo.rcWork;
        monitors.push(RawMonitor {
            device: wide_to_string(&info.szDevice),
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
            work_y: work.top,
            work_height: work.bottom - work.top,
            dpi,
        });
        windows::core::BOOL(1)
    }
    let mut raw: Vec<RawMonitor> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            MONITORENUMPROC::Some(collect),
            LPARAM(&mut raw as *mut _ as isize),
        )
    }
    .ok()
    .map_err(|e| format!("Cannot list displays: {e}"))?;
    if raw.is_empty() {
        return Err("No displays are active.".into());
    }
    Ok(raw)
}

fn wide_to_string(wide: &[u16]) -> String {
    wide.split(|c| *c == 0)
        .next()
        .map(String::from_utf16_lossy)
        .unwrap_or_default()
}

fn topology(raw: &[RawMonitor]) -> String {
    raw.iter()
        .map(|m| {
            format!(
                "{}:{}:{}:{}:{}:{}",
                m.device, m.x, m.y, m.width, m.height, m.dpi
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// The EDID-derived identity and friendly name per GDI device name, from the active
/// display-config paths. A target's `monitorDevicePath` is the per-unit EDID identity
/// that survives reboots and replugs; the GDI name is the fallback when the API fails.
/// Also reports whether any GDI source drives two active targets (duplicate mode).
fn target_identities(raw: &[RawMonitor]) -> (HashMap<String, (String, String)>, bool) {
    let mut map = HashMap::new();
    let mut num_paths = 0u32;
    let mut num_modes = 0u32;
    unsafe {
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut num_paths, &mut num_modes)
            .is_err()
            || num_paths == 0
        {
            return (map, false);
        }
    }
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); num_paths as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); num_modes as usize];
    unsafe {
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut num_paths,
            paths.as_mut_ptr(),
            &mut num_modes,
            modes.as_mut_ptr(),
            None,
        )
        .is_err()
        {
            return (map, false);
        }
    }
    paths.truncate(num_paths as usize);
    let mut source_keys: Vec<(u64, u64, u32)> = Vec::new();
    for path in &paths {
        source_keys.push((
            path.sourceInfo.adapterId.HighPart as u64,
            path.sourceInfo.adapterId.LowPart as u64,
            path.sourceInfo.id,
        ));
        let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: display_header(
                DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                path.sourceInfo.adapterId,
                path.sourceInfo.id,
            ),
            ..Default::default()
        };
        if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != 0 {
            continue;
        }
        let gdi = wide_to_string(&source.viewGdiDeviceName).to_ascii_lowercase();
        if gdi.is_empty()
            || !raw
                .iter()
                .any(|monitor| monitor.device.to_ascii_lowercase() == gdi)
            || map.contains_key(&gdi)
        {
            continue;
        }
        let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
            header: display_header(
                DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                path.targetInfo.adapterId,
                path.targetInfo.id,
            ),
            ..Default::default()
        };
        if unsafe { DisplayConfigGetDeviceInfo(&mut target.header) } != 0 {
            continue;
        }
        let unit = wide_to_string(&target.monitorDevicePath);
        let name = wide_to_string(&target.monitorFriendlyDeviceName);
        let id = if unit.is_empty() {
            gdi.clone()
        } else {
            unit.to_ascii_lowercase()
        };
        map.insert(gdi, (id, name));
    }
    (map, paths_duplicated(&source_keys))
}

/// Whether any source key appears on two active paths (a cloned output renders one GDI
/// desktop on two physical targets).
fn paths_duplicated(source_keys: &[(u64, u64, u32)]) -> bool {
    let mut keys = source_keys.to_vec();
    keys.sort_unstable();
    keys.dedup();
    keys.len() != source_keys.len()
}

fn display_header(
    kind: DISPLAYCONFIG_DEVICE_INFO_TYPE,
    adapter: windows::Win32::Foundation::LUID,
    id: u32,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: kind,
        size: std::mem::size_of::<DISPLAYCONFIG_DEVICE_INFO_HEADER>() as u32,
        adapterId: adapter,
        id,
    }
}

/// Physical pixels -> the Display model, in points at the monitor's own scale.
fn to_display(monitor: &RawMonitor, id: &str, name: &str) -> Display {
    let scale = f64::from(monitor.dpi) / 96.0;
    Display {
        id: id.to_string(),
        name: name.to_string(),
        x: f64::from(monitor.x) / scale,
        y: f64::from(monitor.y) / scale,
        width: f64::from(monitor.width) / scale,
        height: f64::from(monitor.height) / scale,
        work_y: f64::from(monitor.work_y) / scale,
        work_height: f64::from(monitor.work_height) / scale,
        scale,
        mirrored: false,
    }
}

/// Two active monitors sharing one desktop rect render the same picture (duplicate mode);
/// the same GDI source driving two targets says the same thing more reliably.
fn apply_mirroring(displays: &mut [Display], duplicated_sources: bool) {
    if duplicated_sources {
        for display in &mut *displays {
            display.mirrored = true;
        }
        return;
    }
    for i in 0..displays.len() {
        if displays
            .iter()
            .any(|other| other.id != displays[i].id && shares_rect(other, &displays[i]))
        {
            displays[i].mirrored = true;
        }
    }
}

fn shares_rect(a: &Display, b: &Display) -> bool {
    a.x == b.x && a.y == b.y && a.width == b.width && a.height == b.height
}

/// Fallback mirroring signal: two enumerated monitors describing the same desktop rect.
fn shared_rects(raw: &[RawMonitor]) -> bool {
    for (index, monitor) in raw.iter().enumerate() {
        if raw.iter().enumerate().any(|(other, candidate)| {
            other != index
                && candidate.x == monitor.x
                && candidate.y == monitor.y
                && candidate.width == monitor.width
                && candidate.height == monitor.height
        }) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests;
