use chrono::SecondsFormat;

use super::*;
use crate::paths::scratch_dir;

fn write_history(home: &Path, body: &str) -> PathBuf {
    let path = history_path_for_home(home);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, body).unwrap();
    path
}

#[test]
fn latest_sample_is_selected_only_by_exact_organization_id() {
    let home = scratch_dir("claude-desktop-history");
    let path = write_history(
        &home,
        r#"{"version":2,"samples":[
                {"t":1000,"org":"org-1","u":{"fh":1,"sd":2}},
                {"t":3000,"org":"org-2","u":{"fh":90,"sd":91}},
                {"t":2000,"org":"org-1","u":{"fh":3,"sd":4,"unknown":99}}
            ]}"#,
    );

    let usage = read_latest(&path, "org-1").unwrap();
    assert_eq!(
        usage
            .observed_at
            .to_rfc3339_opts(SecondsFormat::Millis, true),
        "1970-01-01T00:00:02.000Z"
    );
    assert_eq!(
        usage
            .windows
            .iter()
            .map(|window| (window.id.as_str(), window.used_percent))
            .collect::<Vec<_>>(),
        [("weekly_all", 4.0), ("session", 3.0)]
    );
    assert!(usage
        .windows
        .iter()
        .all(|window| window.resets_at.is_none()));
    assert!(read_latest(&path, "ORG-1").is_none());
}

#[test]
fn malformed_unknown_version_and_unrecognized_usage_are_ignored() {
    let home = scratch_dir("claude-desktop-history");
    let path = write_history(&home, "{broken");
    assert!(read_latest(&path, "org-1").is_none());

    fs::write(
        &path,
        r#"{"version":3,"samples":[{"t":1000,"org":"org-1","u":{"fh":5}}]}"#,
    )
    .unwrap();
    assert!(read_latest(&path, "org-1").is_none());

    fs::write(
        &path,
        r#"{"version":2,"samples":[{"t":1000,"org":"org-1","u":{"other":5}}]}"#,
    )
    .unwrap();
    assert!(read_latest(&path, "org-1").is_none());
}

#[test]
fn platform_paths_use_the_exact_desktop_data_locations() {
    let home = Path::new("fixture-home");
    let app_data = Path::new("redirected-app-data");
    assert_eq!(
        resolve_history_path(home, None, true, Platform::MacOs),
        home.join("Library")
            .join("Application Support")
            .join("Claude")
            .join(FILE_NAME)
    );
    assert_eq!(
        resolve_history_path(home, Some(app_data), false, Platform::Windows),
        app_data.join("Claude").join(FILE_NAME)
    );
    assert_eq!(
        resolve_history_path(home, Some(app_data), true, Platform::Windows),
        home.join("AppData")
            .join("Roaming")
            .join("Claude")
            .join(FILE_NAME)
    );
    assert_eq!(
        resolve_history_path(home, None, false, Platform::Windows),
        home.join("AppData")
            .join("Roaming")
            .join("Claude")
            .join(FILE_NAME)
    );
}
