use super::*;

#[test]
fn authorization_errors_never_count_as_a_grant() {
    assert_eq!(authorization_result(true, false), Ok(true));
    assert_eq!(authorization_result(false, false), Ok(false));
    assert!(authorization_result(true, true).is_err());
}

/// The names are what the platform APIs accept verbatim (`NSUserNotification.soundName`, the
/// toast sound names `tauri-winrt-notification` parses); a typo would mean a silent notification.
#[cfg(target_os = "macos")]
#[test]
fn every_sound_names_a_macos_system_sound() {
    assert_eq!(Sound::Default.name(), "NSUserNotificationDefaultSoundName");
    assert_eq!(Sound::Success.name(), "Glass");
    assert_eq!(Sound::Done.name(), "Hero");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn every_sound_names_a_windows_toast_sound() {
    assert_eq!(Sound::Default.name(), "Default");
    assert_eq!(Sound::Success.name(), "IM");
    assert_eq!(Sound::Done.name(), "Mail");
}
