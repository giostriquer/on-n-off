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
