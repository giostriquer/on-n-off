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
