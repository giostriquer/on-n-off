use super::*;

#[test]
fn strip_verbatim_drops_windows_prefix() {
    assert_eq!(
        strip_verbatim(r"\\?\C:\Users\me\.codex\.tmp\bundled"),
        PathBuf::from(r"C:\Users\me\.codex\.tmp\bundled")
    );
    assert_eq!(
        strip_verbatim(" /tmp/market "),
        PathBuf::from("/tmp/market")
    );
}
