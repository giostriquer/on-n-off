use super::*;
use crate::cli_stub::CliStub;
use crate::paths::scratch_dir;

/// Windows reports launchers with PATHEXT's casing (`.CMD`), so compare case-insensitively there.
fn assert_launcher(found: Option<PathBuf>, expected: &Path) {
    let found = found.expect("launcher found");
    let same = if cfg!(windows) {
        found
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected.to_string_lossy())
    } else {
        found == expected
    };
    assert!(
        same,
        "expected {}, got {}",
        expected.display(),
        found.display()
    );
}

#[test]
fn cursor_ignores_another_products_agent_command() {
    // `agent` on PATH from an unrelated CLI (e.g. ~/.grok/bin/agent) must not count as Cursor.
    let grok = scratch_dir("on-n-off-grok").join("bin");
    CliStub::new("agent").write(&grok);
    let cursor = scratch_dir("on-n-off-local").join("cursor-agent");
    let launcher = CliStub::new("agent").write(&cursor);

    assert_launcher(find_cursor_cli(&[grok.clone(), cursor.clone()]), &launcher);
    assert_eq!(find_cursor_cli(std::slice::from_ref(&grok)), None);
}

#[test]
fn cursor_accepts_the_legacy_cursor_agent_launcher_anywhere() {
    let bin = scratch_dir("on-n-off-legacy-bin");
    let legacy = CliStub::new("cursor-agent").write(&bin);
    assert_launcher(find_cursor_cli(std::slice::from_ref(&bin)), &legacy);
}

#[test]
fn cursor_prefers_the_canonical_agent_next_to_the_legacy_alias() {
    let cursor = scratch_dir("on-n-off-cursor-install").join("cursor-agent");
    let agent = CliStub::new("agent").write(&cursor);
    CliStub::new("cursor-agent").write(&cursor);
    assert_launcher(find_cursor_cli(std::slice::from_ref(&cursor)), &agent);
}

#[cfg(unix)]
#[test]
fn cursor_follows_the_installers_symlink_into_its_versions_folder() {
    // `~/.local/bin/agent -> ~/.local/share/cursor-agent/versions/<v>/cursor-agent`.
    let home = scratch_dir("on-n-off-cursor-home");
    let versions = home
        .join(".local")
        .join("share")
        .join("cursor-agent")
        .join("versions")
        .join("2026.08.11");
    let real = CliStub::new("cursor-agent").write(&versions);
    let bin = home.join(".local").join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let link = bin.join("agent");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert_eq!(find_cursor_cli(std::slice::from_ref(&bin)), Some(link));
}

#[test]
fn provider_resolution_only_special_cases_cursor() {
    let dir = scratch_dir("on-n-off-provider-bin");
    let claude = CliStub::new("claude").write(&dir);
    CliStub::new("agent").write(&dir);
    assert_launcher(
        resolve_provider_cli_in(AgentId::Claude, "claude", std::slice::from_ref(&dir)),
        &claude,
    );
    assert_eq!(
        resolve_provider_cli_in(AgentId::Cursor, "agent", std::slice::from_ref(&dir)),
        None,
        "a bare `agent` outside a Cursor install folder is not Cursor"
    );
}

#[test]
fn registered_path_values_expand_variables_and_skip_empties() {
    let lookup = |name: &str| match name {
        "SystemRoot" => Some(OsString::from(if cfg!(windows) {
            r"C:\Windows"
        } else {
            "/windows"
        })),
        _ => None,
    };
    let sep = if cfg!(windows) { ';' } else { ':' };
    let values = [
        OsString::from(format!(
            "%SystemRoot%{sep2}system32{sep}{sep}%Missing%{sep2}bin{sep}plain",
            sep2 = std::path::MAIN_SEPARATOR
        )),
        OsString::from("other"),
    ];
    assert_eq!(
        registered_path_dirs_from(&values, lookup),
        vec![
            PathBuf::from(if cfg!(windows) {
                r"C:\Windows\system32".to_string()
            } else {
                "/windows/system32".to_string()
            }),
            PathBuf::from("plain"),
            PathBuf::from("other"),
        ],
        "unexpandable entries are dropped, empties skipped, order kept"
    );
}

#[test]
fn windows_well_known_dirs_include_the_github_cli_installers() {
    let home = PathBuf::from("home");
    let lookup = |name: &str| -> Option<OsString> {
        match name {
            "ProgramFiles" => Some(OsString::from("pf")),
            "LOCALAPPDATA" => Some(OsString::from("local")),
            _ => None,
        }
    };
    let dirs = windows_cli_dirs_from(&home, &lookup);
    assert!(
        dirs.contains(&PathBuf::from("pf").join("GitHub CLI")),
        "the MSI / machine-scope winget install: {dirs:?}"
    );
    assert!(
        dirs.contains(&PathBuf::from("local").join("Programs").join("GitHub CLI")),
        "the user-scope winget install: {dirs:?}"
    );
    let without = windows_cli_dirs_from(&home, &|_: &str| None);
    assert!(
        !without.iter().any(|dir| dir.ends_with("GitHub CLI")),
        "no guessed roots when the variables are unset: {without:?}"
    );
}

#[cfg(windows)]
#[test]
fn windows_well_known_dirs_include_native_provider_installers() {
    let local = PathBuf::from(env::var("LOCALAPPDATA").expect("LOCALAPPDATA"));
    let dirs = well_known_cli_dirs();
    assert!(dirs.contains(&local.join("cursor-agent")), "{dirs:?}");
    assert!(dirs.contains(&local.join("agy").join("bin")), "{dirs:?}");
}

#[test]
fn search_path_keeps_tier_order_and_dedupes() {
    let process = env::join_paths([
        PathBuf::from("/usr/bin"),
        PathBuf::from(""),
        PathBuf::from("/bin"),
    ])
    .unwrap();
    let login = vec![PathBuf::from("/opt/node/bin"), PathBuf::from("/usr/bin")];
    let well_known = vec![PathBuf::from("/opt/node/bin"), PathBuf::from("/opt/tools")];
    assert_eq!(
        merge_search_path(Some(process), &login, well_known),
        vec![
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/opt/node/bin"),
            PathBuf::from("/opt/tools"),
        ]
    );
    assert_eq!(
        merge_search_path(None, &[], vec![PathBuf::from("/x")]),
        vec![PathBuf::from("/x")]
    );
}

#[cfg(unix)]
#[test]
fn search_path_dirs_are_consulted_after_explicit_and_override_paths() {
    let dir = scratch_dir("on-n-off-search-dirs");
    let tool = CliStub::new("on-n-off-search-tool").write(&dir);
    assert_eq!(
        resolve_cli_binary_in("on-n-off-search-tool", std::slice::from_ref(&dir)),
        Some(tool)
    );
    assert_eq!(resolve_cli_binary_in("on-n-off-search-tool", &[]), None);
}

#[cfg(unix)]
#[test]
fn unix_skips_files_without_the_executable_bit() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch_dir("on-n-off-cli-noexec");
    let shim = dir.join("claude");
    std::fs::write(&shim, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        find_in_dirs("claude", std::slice::from_ref(&dir)).is_none(),
        "a non-executable file is not a launcher"
    );
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        find_in_dirs("claude", std::slice::from_ref(&dir)),
        Some(shim)
    );
}

#[cfg(unix)]
#[test]
fn unix_well_known_dirs_cover_homebrew_and_node_version_managers() {
    let home = scratch_dir("on-n-off-home");
    for version in ["v20.11.1", "v24.19.0", "v9.0.0"] {
        std::fs::create_dir_all(
            home.join(".nvm")
                .join("versions")
                .join("node")
                .join(version)
                .join("bin"),
        )
        .unwrap();
    }
    let dirs = well_known_cli_dirs_for(&home);
    for expected in [
        home.join(".local").join("bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".bun").join("bin"),
        home.join(".volta").join("bin"),
    ] {
        assert!(dirs.contains(&expected), "{dirs:?} lacks {expected:?}");
    }
    let node_root = home.join(".nvm").join("versions").join("node");
    let nvm_bins: Vec<&PathBuf> = dirs
        .iter()
        .filter(|dir| dir.starts_with(&node_root))
        .collect();
    assert_eq!(
        nvm_bins,
        vec![
            &node_root.join("v24.19.0").join("bin"),
            &node_root.join("v20.11.1").join("bin"),
            &node_root.join("v9.0.0").join("bin"),
        ],
        "newest node first"
    );
}

#[test]
fn login_shell_prefers_an_existing_shell_env() {
    let dir = scratch_dir("on-n-off-shell-env");
    let shell = dir.join("myshell");
    std::fs::write(&shell, "").unwrap();
    assert_eq!(
        login_shell_from(Some(shell.clone().into_os_string())),
        shell
    );
    let fallback = PathBuf::from(if cfg!(target_os = "macos") {
        "/bin/zsh"
    } else {
        "/bin/sh"
    });
    assert_eq!(
        login_shell_from(Some(dir.join("missing").into_os_string())),
        fallback
    );
    assert_eq!(login_shell_from(None), fallback);
}

#[test]
fn parses_path_from_login_shell_probe_output() {
    let dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/Users/me/.nvm/versions/node/v24.19.0/bin"),
        PathBuf::from("/usr/bin"),
    ];
    let path_value = env::join_paths(&dirs).unwrap();
    let noisy = format!(
        "Welcome banner\n{ENV_START}\nHOME=/Users/me\nMANPATH=/x/man\nPATH={}\nSHELL=/bin/zsh\n{ENV_END}\ntrailing",
        path_value.to_string_lossy()
    );
    assert_eq!(parse_login_shell_path(&noisy), Some(dirs));
    assert_eq!(
        parse_login_shell_path(&format!("{ENV_START}\nPATH=\n{ENV_END}\n")),
        None
    );
    assert_eq!(parse_login_shell_path("PATH=/usr/bin\n"), None);
    assert_eq!(
        parse_login_shell_path(&format!("{ENV_START}\nHOME=/x\n{ENV_END}\n")),
        None
    );
}

#[cfg(unix)]
#[test]
fn probes_the_login_shell_for_path_and_gives_up_on_hangs() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;
    let dir = scratch_dir("on-n-off-shell");
    let write_shell = |name: &str, body: &str| {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    };
    // Stands in for `zsh -i -l -c <cmd>`: rc files put node on PATH, then run the command.
    let shell = write_shell(
        "fakeshell",
        "#!/bin/sh\nPATH=/fake/node/bin:/usr/bin\nexport PATH\neval \"$4\"\n",
    );
    assert_eq!(
        probe_login_shell_path(&shell, Duration::from_secs(5)),
        Some(vec![
            PathBuf::from("/fake/node/bin"),
            PathBuf::from("/usr/bin")
        ])
    );

    let hung = write_shell("hangshell", "#!/bin/sh\nsleep 30\n");
    let started = Instant::now();
    assert_eq!(
        probe_login_shell_path(&hung, Duration::from_millis(200)),
        None
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "probe gave up after {:?}",
        started.elapsed()
    );
}

#[cfg(windows)]
#[test]
fn windows_prefers_cmd_over_extensionless_nvm_shim() {
    let dir = scratch_dir("on-n-off-cli-shim");
    std::fs::write(dir.join("claude"), "#!/usr/bin/env node\n").unwrap();
    std::fs::write(dir.join("claude.cmd"), "@echo off\r\n").unwrap();
    let found = find_in_dirs("claude", std::slice::from_ref(&dir)).expect("cmd launcher");
    let name = found.file_name().unwrap().to_string_lossy();
    assert!(
        name.eq_ignore_ascii_case("claude.cmd"),
        "expected claude.cmd, got {name}"
    );
}
