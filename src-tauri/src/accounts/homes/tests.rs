use super::*;

#[test]
fn a_home_is_found_by_the_id_on_n_off_gave_it_under_the_accounts_directory() {
    let id = "0f8a3c1e-9a52-4d5e-8f6b-2c7d9e0a1b34";

    let found = dir(Path::new("/home"), id).unwrap();

    assert_eq!(found, Path::new("/home/.on-n-off/accounts/homes").join(id));
}

#[test]
fn a_home_id_on_n_off_did_not_make_is_refused() {
    for id in ["", "..", "../../elsewhere", "profile", "/tmp"] {
        assert!(dir(Path::new("/home"), id).is_err(), "{id:?}");
    }
}

fn written(dir: &Path, record: &[u8]) {
    std::fs::write(dir.join("provider.json"), record).unwrap();
}

fn with_layouts(dir: &Path, layouts: &[&str]) {
    for layout in layouts {
        std::fs::create_dir_all(dir.join(layout)).unwrap();
    }
}

#[test]
fn a_home_that_records_its_provider_is_that_providers_whatever_its_layout_or_profile() {
    let home = tempfile::tempdir().unwrap();
    written(home.path(), b"\"muse\"");
    with_layouts(home.path(), &[".claude"]);

    assert_eq!(owner(home.path(), Claim::Unnamed), Some(AgentId::Muse));
    assert_eq!(
        owner(home.path(), Claim::Named(AgentId::Claude)),
        Some(AgentId::Muse)
    );
}

#[test]
fn a_home_with_no_record_is_the_provider_whose_profile_names_it() {
    let home = tempfile::tempdir().unwrap();
    with_layouts(home.path(), &["config/muse"]);

    assert_eq!(
        owner(home.path(), Claim::Named(AgentId::Claude)),
        Some(AgentId::Claude)
    );
    assert_eq!(
        owner(&home.path().join("never-made"), Claim::Named(AgentId::Muse)),
        Some(AgentId::Muse)
    );
}

#[test]
fn an_unnamed_home_with_no_record_is_the_one_provider_whose_layout_it_holds() {
    let owned = |layouts: &[&str]| {
        let home = tempfile::tempdir().unwrap();
        with_layouts(home.path(), layouts);
        owner(home.path(), Claim::Unnamed)
    };

    assert_eq!(owned(&[".claude"]), Some(AgentId::Claude));
    assert_eq!(owned(&["config/muse"]), Some(AgentId::Muse));
    assert_eq!(owned(&[]), None, "no layout");
    assert_eq!(owned(&["config"]), None, "half a layout");
    assert_eq!(owned(&[".claude", "config/muse"]), None, "two layouts");
}

#[test]
fn a_home_whose_record_cannot_be_read_is_no_providers() {
    let mut at_the_cap = b"\"claude\"".to_vec();
    at_the_cap.resize(100, b' ');
    for record in [&b"\"acme\""[..], b"{", &[b' '; 200], &at_the_cap] {
        let home = tempfile::tempdir().unwrap();
        written(home.path(), record);
        with_layouts(home.path(), &[".claude"]);

        assert_eq!(owner(home.path(), Claim::Named(AgentId::Claude)), None);
        assert_eq!(owner(home.path(), Claim::Unnamed), None);
    }
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("provider.json")).unwrap();
    assert_eq!(owner(home.path(), Claim::Named(AgentId::Claude)), None);
}

#[test]
fn a_home_records_its_provider_once_and_refuses_any_other() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");

    record(&home, AgentId::Claude).unwrap();
    record(&home, AgentId::Claude).unwrap();

    assert_eq!(
        std::fs::read_to_string(home.join("provider.json")).unwrap(),
        "\"claude\""
    );
    assert!(record(&home, AgentId::Muse).is_err());
    assert_eq!(recorded_provider(&home), Some(AgentId::Claude));
    written(&home, b"{");
    assert!(
        record(&home, AgentId::Claude).is_err(),
        "a record it cannot read"
    );
    assert_eq!(recorded_provider(&home), None);
}
