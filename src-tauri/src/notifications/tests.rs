use super::*;

#[test]
fn authorization_errors_never_count_as_a_grant() {
    assert_eq!(authorization_result(true, false), Ok(true));
    assert_eq!(authorization_result(false, false), Ok(false));
    assert!(authorization_result(true, true).is_err());
}
