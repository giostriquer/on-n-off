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

fn laid_out(found: &'static [AgentId]) -> impl Fn(AgentId, &Path) -> bool {
    move |provider, _| found.contains(&provider)
}

fn written(dir: &Path, record: &[u8]) {
    std::fs::write(dir.join("provider.json"), record).unwrap();
}

#[test]
fn a_home_that_records_its_provider_is_that_providers_whatever_its_layout_or_profile() {
    let home = tempfile::tempdir().unwrap();
    written(home.path(), b"\"muse\"");

    let claude = laid_out(&[AgentId::Claude]);

    assert_eq!(
        owner(home.path(), Claim::Unnamed(&claude)),
        Some(AgentId::Muse)
    );
    assert_eq!(
        owner(home.path(), Claim::Named(AgentId::Claude)),
        Some(AgentId::Muse)
    );
}

#[test]
fn a_home_with_no_record_is_the_provider_whose_profile_names_it() {
    let home = tempfile::tempdir().unwrap();

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
    let home = tempfile::tempdir().unwrap();
    let owned = |found: &'static [AgentId]| owner(home.path(), Claim::Unnamed(&laid_out(found)));

    assert_eq!(owned(&[AgentId::Claude]), Some(AgentId::Claude));
    assert_eq!(owned(&[AgentId::Muse]), Some(AgentId::Muse));
    assert_eq!(owned(&[]), None, "no layout");
    assert_eq!(
        owned(&[AgentId::Claude, AgentId::Muse]),
        None,
        "two layouts"
    );
}

#[test]
fn a_home_whose_record_cannot_be_read_is_no_providers() {
    let claude = laid_out(&[AgentId::Claude]);
    for record in [&b"\"acme\""[..], b"{", &[b' '; 200]] {
        let home = tempfile::tempdir().unwrap();
        written(home.path(), record);

        assert_eq!(owner(home.path(), Claim::Named(AgentId::Claude)), None);
        assert_eq!(owner(home.path(), Claim::Unnamed(&claude)), None);
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
