use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::dto::AdapterError;

pub async fn request_permission(app: AppHandle) -> Result<bool, AdapterError> {
    #[cfg(target_os = "macos")]
    let answer = macos::center().map(|center| macos::request_authorization(&center));
    #[cfg(target_os = "macos")]
    if let Some(mut answer) = answer {
        return answer
            .recv()
            .await
            .ok_or_else(|| AdapterError::message("macOS did not return notification permission"))?
            .map_err(AdapterError::message);
    }
    request_permission_through_plugin(&app)
}

fn request_permission_through_plugin(app: &AppHandle) -> Result<bool, AdapterError> {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Default,
    Success,
    Done,
}

impl Sound {
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
    if let Some(center) = macos::center() {
        macos::post(&center, &title, &body, sound);
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
    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSBundle, NSError, NSString, NSUUID};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNNotificationSound, UNUserNotificationCenter,
    };

    use tauri::async_runtime::Receiver;

    use super::{authorization_result, Sound};

    pub(super) fn center() -> Option<Retained<UNUserNotificationCenter>> {
        NSBundle::mainBundle().bundleIdentifier()?;
        Some(UNUserNotificationCenter::currentNotificationCenter())
    }

    pub(super) fn request_authorization(
        center: &UNUserNotificationCenter,
    ) -> Receiver<Result<bool, String>> {
        let (sender, receiver) = tauri::async_runtime::channel(1);
        let completion: RcBlock<dyn Fn(Bool, *mut NSError)> =
            RcBlock::new(move |granted: Bool, error: *mut NSError| {
                let result = authorization_result(granted.as_bool(), !error.is_null())
                    .map_err(str::to_string);
                let _ = sender.try_send(result);
            });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &completion,
        );
        receiver
    }

    pub(super) fn post(center: &UNUserNotificationCenter, title: &str, body: &str, sound: Sound) {
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
        let completion: RcBlock<dyn Fn(*mut NSError)> = RcBlock::new(|error: *mut NSError| {
            if !error.is_null() {
                eprintln!("macOS refused a notification");
            }
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&completion));
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
