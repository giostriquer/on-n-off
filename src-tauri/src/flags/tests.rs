use super::*;

#[test]
fn defaults_keep_master_cut_off() {
    let flags = resolve_flags(None, |_| None);
    assert_eq!(flags, FeatureFlags { master_cut: false });
}

#[test]
fn file_can_enable_master_cut() {
    let flags = resolve_flags(Some(r#"{ "masterCut": true, "nope": 1 }"#), |_| None);
    assert!(flags.master_cut);
}

#[test]
fn malformed_file_falls_back_to_defaults() {
    let flags = resolve_flags(Some("{not json"), |_| None);
    assert!(!flags.master_cut);
}

#[test]
fn env_overrides_file() {
    let flags = resolve_flags(Some(r#"{ "masterCut": true }"#), |key| {
        (key == "MASTER_CUT").then(|| "0".into())
    });
    assert!(!flags.master_cut);
}

#[test]
fn env_parses_common_bool_tokens() {
    assert_eq!(parse_env_bool("TRUE"), Some(true));
    assert_eq!(parse_env_bool(" yes "), Some(true));
    assert_eq!(parse_env_bool("off"), Some(false));
    assert_eq!(parse_env_bool("maybe"), None);
}
