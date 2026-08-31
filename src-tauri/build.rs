#[path = "src/updater_build.rs"]
mod updater_build;

mod native_build;

fn main() {
    native_build::build();
    println!("cargo:rerun-if-env-changed=ON_N_OFF_INSTALLER_KIND");
    let raw_kind = std::env::var("ON_N_OFF_INSTALLER_KIND").ok();
    let kind = updater_build::parse_installer_kind(raw_kind.as_deref())
        .unwrap_or_else(|error| panic!("{error}"));

    if let (Some(name), Some(target)) = (kind.name(), kind.target()) {
        println!("cargo:rustc-env=ON_N_OFF_UPDATER_KIND={name}");
        println!("cargo:rustc-env=ON_N_OFF_UPDATER_TARGET={target}");
    }

    tauri_build::build()
}
