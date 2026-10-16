#![allow(unsafe_code)]

use super::host::frame::RailData;
use super::host::{self, Action, Inbound, Link, Transport, STOPPED};
use super::model::NotchSnapshot;
use super::win_window::{WinAction, WindowMsg};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::Instant;
use tauri::AppHandle;

#[derive(Default)]
struct State {
    stopped: bool,
    window: Option<mpsc::Sender<WindowMsg>>,
}

#[derive(Default)]
pub struct Lifetime(Mutex<State>);

impl Lifetime {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }
}

impl host::Lifetime for Lifetime {
    fn stopped(&self) -> bool {
        self.state().stopped
    }

    fn shutdown(&self) {
        let mut state = self.state();
        state.stopped = true;
        if let Some(window) = state.window.take() {
            let _ = window.send(WindowMsg::Shutdown);
        }
    }
}

struct Window {
    messages: mpsc::Sender<WindowMsg>,
    actions: mpsc::Receiver<WinAction>,
}

type Opener = Box<dyn Fn(&str) -> Result<(), String> + Send>;
type Copier = fn(&str, &str) -> Result<(), String>;

pub struct Overlay {
    open_url: Opener,
    copy: Copier,
    lifetime: Arc<Lifetime>,
    window: Option<Window>,
}

impl Overlay {
    pub fn new(app: AppHandle) -> (Self, Arc<Lifetime>) {
        Self::with(
            Box::new(move |url| open_in_browser(&app, url)),
            copy_review_request,
        )
    }

    fn with(open_url: Opener, copy: Copier) -> (Self, Arc<Lifetime>) {
        let lifetime = Arc::new(Lifetime::default());
        let overlay = Self {
            open_url,
            copy,
            lifetime: lifetime.clone(),
            window: None,
        };
        (overlay, lifetime)
    }

    fn lost(&mut self) -> String {
        self.window = None;
        self.lifetime.state().window = None;
        STOPPED.into()
    }

    fn answer(&self, action: WinAction) -> Inbound {
        match action {
            WinAction::Refresh => Inbound::Action(Action::Refresh),
            WinAction::OpenLimits => Inbound::Action(Action::OpenLimits),
            WinAction::OpenPullRequests => Inbound::Action(Action::OpenPullRequests),
            WinAction::SetShow(show) => Inbound::Action(Action::SetShow(show)),
            WinAction::OpenUrl(url) if crate::item_install::is_openable_url(&url) => {
                Inbound::Handled((self.open_url)(&url))
            }
            WinAction::OpenUrl(url) => Inbound::Handled(Err(format!("refusing to open {url}"))),
            WinAction::CopyReviewRequest { title, url } => {
                Inbound::Handled((self.copy)(&title, &url))
            }
        }
    }
}

fn open_in_browser(app: &AppHandle, url: &str) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

impl Transport for Overlay {
    fn connect(&mut self, _: Instant) -> Result<Link, String> {
        if self.window.is_some() {
            return Ok(Link::Open);
        }
        let mut state = self.lifetime.state();
        if state.stopped {
            return Err("Native notch is shutting down.".into());
        }
        let (action_tx, actions) = mpsc::channel::<WinAction>();
        let messages = super::win_window::spawn(action_tx);
        state.window = Some(messages.clone());
        drop(state);
        self.window = Some(Window { messages, actions });
        Ok(Link::Opened)
    }

    fn receive(&mut self, _: Instant, inbound: &mut Vec<Inbound>) -> Result<(), String> {
        let actions: Vec<WinAction> = match &self.window {
            Some(window) if window.messages.send(WindowMsg::Ping).is_ok() => {
                window.actions.try_iter().collect()
            }
            _ => return Err(self.lost()),
        };
        inbound.extend(actions.into_iter().map(|action| self.answer(action)));
        Ok(())
    }

    fn ready(&self) -> bool {
        true
    }

    fn send(&mut self, _: Instant, _: &NotchSnapshot, frame: RailData) -> Result<(), String> {
        let sent = self
            .window
            .as_ref()
            .is_some_and(|window| window.messages.send(WindowMsg::Data(frame)).is_ok());
        if sent {
            Ok(())
        } else {
            Err(self.lost())
        }
    }

    fn close(&mut self) {
        if let Some(window) = self.window.take() {
            let _ = window.messages.send(WindowMsg::Shutdown);
        }
        self.lifetime.state().window = None;
    }

    fn settings_changed(&mut self) {}
}

fn copy_review_request(title: &str, url: &str) -> Result<(), String> {
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        OpenClipboard(None).map_err(|e| e.to_string())?;
        let result = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| e.to_string())?;
            let text = format!("review please: {title} {url}\0");
            let bytes: Vec<u16> = text.encode_utf16().collect();
            let handle: HGLOBAL =
                GlobalAlloc(GMEM_MOVEABLE, bytes.len() * 2).map_err(|e| e.to_string())?;
            let locked = GlobalLock(handle);
            if locked.is_null() {
                let _ = GlobalUnlock(handle);
                let _ = GlobalFree(Some(handle));
                return Err("Cannot lock the clipboard buffer.".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), locked as *mut u16, bytes.len());
            let _ = GlobalUnlock(handle);
            match SetClipboardData(CF_UNICODETEXT, Some(HANDLE(handle.0))) {
                Ok(_) => Ok(()),
                Err(error) => {
                    let _ = GlobalFree(Some(handle));
                    Err(error.to_string())
                }
            }
        })();
        let _ = CloseClipboard();
        result
    }
}

#[cfg(test)]
mod tests;
