use super::*;

#[test]
fn authorization_errors_never_count_as_a_grant() {
    assert_eq!(authorization_result(true, false), Ok(true));
    assert_eq!(authorization_result(false, false), Ok(false));
    assert!(authorization_result(true, true).is_err());
}

#[test]
fn every_sound_names_a_distinct_platform_sound() {
    let names: Vec<&str> = Sound::ALL.iter().map(|sound| sound.name()).collect();
    assert!(names.iter().all(|name| !name.is_empty()), "{names:?}");
    for (index, name) in names.iter().enumerate() {
        assert!(!names[..index].contains(name), "{name} is used twice");
    }
}
