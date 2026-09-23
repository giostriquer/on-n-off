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
    // Package.swift links Info.plist into the helper with a `-sectcreate` flag, and SwiftPM's
    // native build system does not track a file that only a flag names. After a change to
    // Info.plist alone it calls the helper up to date and keeps the old plist inside the Mach-O,
    // while the bundle below gets the new Contents/Info.plist. CI restores `.build` from a cache,
    // so every release's version bump would hit that. Removing the helper makes the build link
    // it again, which takes about half a second. Swift Build, the default from Swift 6.4, tracks
    // the file and puts the helper elsewhere. If an older toolchain left one at this path,
    // removing it also means `helper_binary`'s fallback can only find a helper this build linked.
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

/// Where SwiftPM left the product, asked rather than assumed.
///
/// Swift 6.4 moved a package build to the Xcode-style layout, so the helper that used to land in
/// `.build/<arch>-apple-macosx/<configuration>/` now lands in `.build/out/Products/<Configuration>/`.
/// A hard-coded path meant the build script panicked staging a helper the Swift build had just
/// produced, which broke every local Rust command on a current toolchain while CI, still on an
/// older one, stayed green. `--show-bin-path` answers for whichever layout is in force, and it has
/// to repeat the build's own flags or it answers for a different variant. The old path stays as a
/// fallback in case a toolchain declines the query, and a failure names both so the next person
/// does not have to guess which layout they are on.
///
/// The fallback still warns: a toolchain that declines the query is one this script was not
/// written against. `build` removes the helper at the old path before building, so whatever the
/// fallback finds there was linked by this build, not left behind by an earlier one.
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

/// Unlink before copying: overwriting a helper in place while an app still has it running
/// leaves a file macOS refuses to launch (killed at exec) until it is recreated.
fn replace_file(source: &Path, destination: &Path, what: &str) {
    let _ = fs::remove_file(destination);
    fs::copy(source, destination).expect(what);
}
