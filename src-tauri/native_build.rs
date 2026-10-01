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
    let package = root.join("macos/SideNotch");
    println!("cargo:rerun-if-changed=macos/SideNotch/Package.swift");
    println!("cargo:rerun-if-changed=macos/SideNotch/Sources");
    println!("cargo:rerun-if-changed=macos/SideNotch/Info.plist");
    let target = env::var("TARGET").expect("target triple");
    let arch = if target.starts_with("aarch64-") {
        "arm64"
    } else {
        "x86_64"
    };
    let configuration = if env::var("PROFILE").as_deref() == Ok("release") {
        "release"
    } else {
        "debug"
    };
    let legacy = package.join(format!(
        ".build/{arch}-apple-macosx/{configuration}/on-n-off-notch"
    ));
    let _ = fs::remove_file(&legacy);
    let output = Command::new("/usr/bin/xcrun")
        .args(["swift", "build", "--package-path"])
        .arg(&package)
        .args([
            "--configuration",
            configuration,
            "--arch",
            arch,
            "--product",
            "on-n-off-notch",
            "-Xswiftc",
            "-warnings-as-errors",
        ])
        .output()
        .expect("Swift Command Line Tools are required to build the macOS notch");
    assert!(
        output.status.success(),
        "Native notch build failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = helper_binary(&package, arch, configuration, legacy);
    let binaries = root.join("binaries");
    fs::create_dir_all(&binaries).expect("create native binaries directory");
    let helper = binaries.join("on-n-off-notch.app");
    let contents = helper.join("Contents");
    fs::create_dir_all(contents.join("MacOS")).expect("create native helper bundle");
    replace_file(
        &binary,
        &contents.join("MacOS/on-n-off-notch"),
        "stage native helper",
    );
    fs::copy(package.join("Info.plist"), contents.join("Info.plist"))
        .expect("stage helper metadata");
    let signed = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-"])
        .arg(&helper)
        .output()
        .expect("sign native helper");
    assert!(
        signed.status.success(),
        "Native helper signing failed: {}",
        String::from_utf8_lossy(&signed.stderr)
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("build output"));
    let profile = out.ancestors().nth(3).expect("cargo profile directory");
    replace_file(
        &binary,
        &profile.join("on-n-off-notch"),
        "stage development native notch",
    );
}

fn helper_binary(package: &Path, arch: &str, configuration: &str, legacy: PathBuf) -> PathBuf {
    let reported = Command::new("/usr/bin/xcrun")
        .args(["swift", "build", "--package-path"])
        .arg(package)
        .args([
            "--configuration",
            configuration,
            "--arch",
            arch,
            "--show-bin-path",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let line = stdout
                .lines()
                .map(str::trim)
                .rfind(|line| !line.is_empty())?;
            Some(PathBuf::from(line).join("on-n-off-notch"))
        });
    if let Some(reported) = reported {
        if reported.is_file() {
            return reported;
        }
    }
    println!(
        "cargo:warning=Swift did not report a usable build path for the notch helper; \
         falling back to {}. If that file is stale, the bundled helper will be too.",
        legacy.display()
    );
    assert!(
        legacy.is_file(),
        "The native notch helper is missing. Swift reported no usable path and it is not at {}.",
        legacy.display()
    );
    legacy
}

fn replace_file(source: &Path, destination: &Path, what: &str) {
    let _ = fs::remove_file(destination);
    fs::copy(source, destination).expect(what);
}
