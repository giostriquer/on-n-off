//! Build the macOS-only cookie reader using the same Swift package as CodexBar.
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
pub fn build() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let package = root.join("macos/BrowserBilling");
    for input in ["Package.swift", "Package.resolved", "Sources"] {
        println!("cargo:rerun-if-changed=macos/BrowserBilling/{input}");
    }
    let configuration = if env::var("PROFILE").as_deref() == Ok("release") {
        "release"
    } else {
        "debug"
    };
    let arch = if env::var("TARGET")
        .unwrap_or_default()
        .starts_with("aarch64-")
    {
        "arm64"
    } else {
        "x86_64"
    };
    let args = [
        "swift",
        "build",
        "--configuration",
        configuration,
        "--arch",
        arch,
        "--package-path",
    ];
    let output = Command::new("/usr/bin/xcrun")
        .args(args)
        .arg(&package)
        .args(["--product", "on-n-off-billing"])
        .output()
        .expect("build browser billing helper");
    assert!(
        output.status.success(),
        "Browser billing build failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // Ask SwiftPM for its output directory; it varies between Swift build engines.
    let output = Command::new("/usr/bin/xcrun")
        .args(args)
        .arg(&package)
        .arg("--show-bin-path")
        .output()
        .expect("locate browser billing helper");
    assert!(
        output.status.success(),
        "Cannot locate browser billing build output"
    );
    let folder = PathBuf::from(String::from_utf8(output.stdout).expect("Swift path").trim());
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("build output"));
    let profile = out.ancestors().nth(3).expect("cargo profile");
    for destination in [root.join("binaries"), profile.to_path_buf()] {
        fs::create_dir_all(&destination).expect("create helper directory");
        let binary = destination.join("on-n-off-billing");
        let _ = fs::remove_file(&binary);
        fs::copy(folder.join("on-n-off-billing"), &binary).expect("stage billing helper");
        let resource = "BrowserBilling_BrowserBilling.bundle";
        copy_tree(&folder.join(resource), &destination.join(resource));
        let signed = Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(&binary)
            .output()
            .expect("sign billing helper");
        assert!(signed.status.success(), "Cannot sign billing helper");
    }
}
fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create resource directory");
    for entry in fs::read_dir(source).expect("read billing resources") {
        let entry = entry.expect("resource entry");
        let target = destination.join(entry.file_name());
        if entry.file_type().expect("resource type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy billing resource");
        }
    }
}
