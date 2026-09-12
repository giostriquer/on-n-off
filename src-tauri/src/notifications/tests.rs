use super::*;

#[test]
fn authorization_errors_never_count_as_a_grant() {
    assert_eq!(authorization_result(true, false), Ok(true));
    assert_eq!(authorization_result(false, false), Ok(false));
    assert!(authorization_result(true, true).is_err());
}

/// The names are what the platform APIs accept verbatim; a typo would mean a silent notification.
/// On macOS the bundled app posts through UserNotifications, which takes a system sound file
/// name (`None` is the framework's default sound); the unbundled dev build falls back to the
/// plugin's `NSUserNotification` path and its bare names.
#[cfg(target_os = "macos")]
#[test]
fn every_sound_names_a_macos_system_sound() {
    assert_eq!(Sound::Default.file_name(), None);
    assert_eq!(Sound::Success.file_name(), Some("Glass.aiff"));
    assert_eq!(Sound::Done.file_name(), Some("Hero.aiff"));
    assert_eq!(
        Sound::Default.plugin_name(),
        "NSUserNotificationDefaultSoundName"
    );
    assert_eq!(Sound::Success.plugin_name(), "Glass");
    assert_eq!(Sound::Done.plugin_name(), "Hero");
    for sound in [Sound::Success, Sound::Done] {
        let path = std::path::Path::new("/System/Library/Sounds").join(sound.file_name().unwrap());
        assert!(path.is_file(), "{} is not a system sound", path.display());
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn every_sound_names_a_windows_toast_sound() {
    assert_eq!(Sound::Default.plugin_name(), "Default");
    assert_eq!(Sound::Success.plugin_name(), "IM");
    assert_eq!(Sound::Done.plugin_name(), "Mail");
}
