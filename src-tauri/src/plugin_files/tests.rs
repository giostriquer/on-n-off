use super::*;
use crate::paths::scratch_dir;

#[test]
fn resolves_a_manifest_path_inside_the_plugin() {
    let root = scratch_dir("plugin-files-inside");

    let plain = plugin_file(&root, "./servers/extra.json").unwrap();
    let windows = plugin_file(&root, "hooks\\a.json").unwrap();

    assert_eq!(plain.key, "servers/extra.json");
    assert_eq!(plain.path, root.join("servers").join("extra.json"));
    assert_eq!(windows.key, "hooks/a.json");
    assert_eq!(windows.path, root.join("hooks").join("a.json"));
}

/// Decided by the text, the same on every platform: on Windows `\outside.json` and `/outside.json`
/// are not absolute and `C:outside.json` is drive-relative, and each would land outside the plugin
/// once joined.
#[test]
fn refuses_every_path_that_could_leave_the_plugin() {
    let root = scratch_dir("plugin-files-escape");
    let absolute = root
        .parent()
        .unwrap()
        .join("outside.json")
        .to_string_lossy()
        .into_owned();

    for rel in [
        "../outside.json",
        "servers/../../outside.json",
        "/outside.json",
        "\\outside.json",
        "C:outside.json",
        "servers/C:outside.json",
        absolute.as_str(),
        "",
        "./",
    ] {
        assert_eq!(plugin_file(&root, rel), None, "{rel:?}");
    }
}
