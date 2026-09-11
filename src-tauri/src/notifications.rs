//! Desktop notifications. Windows goes through the notification plugin (a toast). macOS is
//! split: the bundled app asks for permission and posts through `UserNotifications`, the
//! framework every app is expected to use since 10.14 — an app that has registered with it
//! (which asking for permission does) no longer gets the deprecated `NSUserNotification` path
//! the plugin uses delivered, and only this path can ask for sound. An unbundled dev build has
//! no bundle for `UserNotifications` to attach to and keeps the plugin.

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::dto::AdapterError;

#[cfg(target_os = "macos")]
pub async fn request_permission(_app: AppHandle) -> Result<bool, AdapterError> {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::NSError;
    use objc2_user_notifications::{UNAuthorizationOptions, UNUserNotificationCenter};
    use tauri::async_runtime;

    let (sender, mut receiver) = async_runtime::channel(1);
    {
        let completion: RcBlock<dyn Fn(Bool, *mut NSError)> =
            RcBlock::new(move |granted: Bool, error: *mut NSError| {
                let result = authorization_result(granted.as_bool(), !error.is_null())
                    .map_err(str::to_string);
                let _ = sender.try_send(result);
            });
        UNUserNotificationCenter::currentNotificationCenter()
            .requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
                &completion,
            );
    }

    receiver
        .recv()
        .await
        .ok_or_else(|| AdapterError::message("macOS did not return notification permission"))?
        .map_err(AdapterError::message)
}

/// Ask again at startup while a notification setting is on. macOS only offers the sound
/// controls once an app has asked for sound, and an authorization already given is refreshed
/// without a prompt, so a login that dates from when the app asked for alerts alone gains the
/// "play sound" switch the first time this version runs.
#[cfg(target_os = "macos")]
pub fn refresh_authorization(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let settings = crate::settings::load_settings();
        if !(settings.limit_notifications || settings.github_notifications) {
            return;
        }
        if let Err(error) = request_permission(app).await {
            eprintln!(
                "could not refresh notification permission: {}",
                error.message
            );
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub async fn request_permission(app: AppHandle) -> Result<bool, AdapterError> {
    use tauri_plugin_notification::PermissionState;

    let notification = app.notification();
    let state = notification
        .permission_state()
        .map_err(|error| AdapterError::message(error.to_string()))?;
    match state {
        PermissionState::Granted => Ok(true),
        PermissionState::Denied => Ok(false),
        PermissionState::Prompt | PermissionState::PromptWithRationale => notification
            .request_permission()
            .map(|state| state == PermissionState::Granted)
            .map_err(|error| AdapterError::message(error.to_string())),
    }
}

/// The sound a notification arrives with. Every notification plays one, so the OS's own
/// per-app "play sound" switch stays the single place to silence them; the two named ones let
/// a monitor mark news worth telling apart without looking (what each means is its call).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// The platform's default notification sound.
    Default,
    /// Something turned out well.
    Success,
    /// Something is finished.
    Done,
}

impl Sound {
    /// The name the notification plugin expects: one of the toast `ms-winsoundevent` names on
    /// Windows, an `NSUserNotification` sound name on a macOS dev build.
    #[cfg(target_os = "macos")]
    fn plugin_name(self) -> &'static str {
        match self {
            Self::Default => "NSUserNotificationDefaultSoundName",
            Self::Success => "Glass",
            Self::Done => "Hero",
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn plugin_name(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Success => "IM",
            Self::Done => "Mail",
        }
    }

    /// The system sound file `UserNotifications` plays, `None` for its default sound.
    #[cfg(target_os = "macos")]
    fn file_name(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Success => Some("Glass.aiff"),
            Self::Done => Some("Hero.aiff"),
        }
    }
}

pub fn show(
    app: &AppHandle,
    title: String,
    body: String,
    sound: Sound,
) -> Result<(), AdapterError> {
    #[cfg(target_os = "macos")]
    if macos::post(&title, &body, sound) {
        return Ok(());
    }
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .sound(sound.plugin_name())
        .show()
        .map_err(|error| AdapterError::message(error.to_string()))
}

#[cfg(target_os = "macos")]
mod macos {
    use block2::RcBlock;
    use objc2_foundation::{NSBundle, NSError, NSString, NSUUID};
    use objc2_user_notifications::{
        UNMutableNotificationContent, UNNotificationRequest, UNNotificationSound,
        UNUserNotificationCenter,
    };

    use super::Sound;

    /// Post through `UserNotifications`. `false` when this process has no bundle identifier
    /// (an unbundled dev build), which the framework cannot serve; the caller falls back.
    pub(super) fn post(title: &str, body: &str, sound: Sound) -> bool {
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            return false;
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        let sound = match sound.file_name() {
            Some(name) => UNNotificationSound::soundNamed(&NSString::from_str(name)),
            None => UNNotificationSound::defaultSound(),
        };
        content.setSound(Some(&sound));
        let identifier = NSUUID::new().UUIDString();
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &identifier,
            &content,
            None,
        );
        // Reading the error would need `unsafe`, which this crate forbids; its presence is
        // the useful part (the app is not allowed to notify, or the framework is unavailable).
        let completion: RcBlock<dyn Fn(*mut NSError)> = RcBlock::new(|error: *mut NSError| {
            if !error.is_null() {
                eprintln!("macOS refused a notification");
            }
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, Some(&completion));
        true
    }
}

#[cfg(any(target_os = "macos", test))]
fn authorization_result(granted: bool, has_error: bool) -> Result<bool, &'static str> {
    if has_error {
        Err("macOS could not request notification permission")
    } else {
        Ok(granted)
    }
}

#[cfg(test)]
mod tests;
