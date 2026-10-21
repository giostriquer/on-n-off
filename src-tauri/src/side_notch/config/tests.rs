use super::*;
#[test]
fn a_saved_document_round_trips_and_advances_the_revision() {
    let dir = crate::paths::scratch_dir("notch-settings");
    let path = dir.join("side-notch.json");
    let counter = Revision::new();
    let current = NotchSettings {
        enabled: true,
        display_id: Some("display-two".into()),
        edge: super::super::model::Edge::Left,
        ..NotchSettings::default()
    };
    assert_eq!(save_to(&path, &current, &counter).unwrap(), 1);
    let persisted: NotchSettings =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(persisted, current);
    assert_eq!(counter.current(), 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_launch_smoke_settings_turn_on_the_notch_with_every_provider_and_pull_request_list() {
    use super::super::model::{GITHUB_LIST_ORDER, RAIL_ORDER};
    let settings: NotchSettings = serde_json::from_str(include_str!(
        "../../../../scripts/fixtures/launch-smoke-side-notch.json"
    ))
    .unwrap();
    assert!(settings.enabled);
    assert_eq!(settings.rail_providers(), RAIL_ORDER.to_vec());
    assert!(settings.pull_requests.enabled);
    assert_eq!(
        settings.pull_requests.selected_lists(),
        GITHUB_LIST_ORDER.to_vec()
    );
}
