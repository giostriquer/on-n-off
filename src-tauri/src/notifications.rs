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
                UNAuthorizationOptions::Alert,
                &completion,
            );
    }

    receiver
        .recv()
        .await
        .ok_or_else(|| AdapterError::message("macOS did not return notification permission"))?
        .map_err(AdapterError::message)
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

pub fn show(app: &AppHandle, title: String, body: String) -> Result<(), AdapterError> {
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|error| AdapterError::message(error.to_string()))
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
mod tests {
    use super::*;

    #[test]
    fn authorization_errors_never_count_as_a_grant() {
        assert_eq!(authorization_result(true, false), Ok(true));
        assert_eq!(authorization_result(false, false), Ok(false));
        assert!(authorization_result(true, true).is_err());
    }
}
