//! Where agent CLIs live.
//!
//! GUI apps do not always inherit the PATH a terminal has: on macOS, Finder launches with a
//! minimal one, so nvm/volta/homebrew shims are invisible. The app therefore searches one
//! merged list — the process PATH, then the login shell's PATH (probed once), then well-known
//! install folders — and hands that same list to spawned CLIs as their PATH, so
//! `#!/usr/bin/env node` launchers keep working wherever they were found.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use crate::dto::{AgentId, AgentInfo};
use crate::paths::user_home;
use crate::process::{wait_with_deadline, CommandOutcome};

/// Upper bound for asking the login shell about its PATH; a hung rc file must not stall the app.
const SHELL_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const ENV_START: &str = "__ON_N_OFF_ENV_START__";
const ENV_END: &str = "__ON_N_OFF_ENV_END__";

/// Every directory searched for agent CLIs, in priority order, computed once per process.
pub fn cli_search_path() -> &'static [PathBuf] {
    static SEARCH_PATH: OnceLock<Vec<PathBuf>> = OnceLock::new();
    SEARCH_PATH.get_or_init(|| {
        merge_search_path(
            env::var_os("PATH"),
            login_shell_path_dirs(),
            well_known_cli_dirs(),
        )
    })
}

/// [`cli_search_path`] joined into a PATH value for child processes; `None` if it cannot be joined.
pub fn cli_search_path_value() -> Option<OsString> {
    env::join_paths(cli_search_path()).ok()
}

fn merge_search_path(
    process_path: Option<OsString>,
    login_shell_dirs: &[PathBuf],
    well_known: Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut merged: Vec<PathBuf> = Vec::new();
    let process_dirs = process_path
        .map(|value| env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_default();
    for dir in process_dirs
        .into_iter()
        .chain(login_shell_dirs.iter().cloned())
        .chain(well_known)
    {
        if !dir.as_os_str().is_empty() && !merged.contains(&dir) {
            merged.push(dir);
        }
    }
    merged
}

pub fn binary_on_path(name: &str) -> bool {
    resolve_cli_binary(name).is_some()
}

/// First launchable CLI file: explicit path, settings override, then [`cli_search_path`].
///
/// On Windows, PATHEXT launchers (`.cmd` / `.exe`) win over extensionless npm/nvm shims.
/// On Unix a candidate must carry the executable bit.
pub fn resolve_cli_binary(name: &str) -> Option<PathBuf> {
    resolve_cli_binary_in(name, cli_search_path())
}

fn resolve_cli_binary_in(name: &str, search_path: &[PathBuf]) -> Option<PathBuf> {
    let candidate = PathBuf::from(name);
    if let Some(path) = launchable(&candidate) {
        return Some(path);
    }
    if let Some(override_path) = crate::settings::binary_override_for(name) {
        if let Some(path) = launchable(&override_path) {
            return Some(path);
        }
    }
    find_in_dirs(name, search_path)
}

pub(crate) fn find_in_dirs(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter().find_map(|dir| launchable(&dir.join(name)))
}

fn pathext() -> Vec<String> {
    if cfg!(windows) {
        env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .map(|ext| ext.trim_start_matches('.').to_string())
            .filter(|ext| !ext.is_empty())
            .collect()
    } else {
        Vec::new()
    }
}

fn has_windows_launcher_ext(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    pathext()
        .iter()
        .any(|known| known.eq_ignore_ascii_case(ext))
}

/// The file the OS can actually spawn for `path`, if any.
///
/// Windows: prefer a Win32 launcher next to an extensionless nvm/npm shim.
/// Unix: the file itself, provided it is executable.
fn launchable(path: &Path) -> Option<PathBuf> {
    if !path.is_file() {
        if cfg!(windows) && path.extension().is_none() {
            for ext in pathext() {
                let mut with_ext = path.to_path_buf();
                with_ext.set_extension(ext);
                if with_ext.is_file() {
                    return Some(with_ext);
                }
            }
        }
        return None;
    }
    if !cfg!(windows) {
        return is_executable(path).then(|| path.to_path_buf());
    }
    if has_windows_launcher_ext(path) {
        return Some(path.to_path_buf());
    }
    for ext in pathext() {
        let mut with_ext = path.to_path_buf();
        with_ext.set_extension(ext);
        if with_ext.is_file() {
            return Some(with_ext);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

/// Install folders that commonly hold agent CLIs but are missing from a GUI app's PATH.
pub fn well_known_cli_dirs() -> Vec<PathBuf> {
    user_home()
        .map(|home| well_known_cli_dirs_for(&home))
        .unwrap_or_default()
}

fn well_known_cli_dirs_for(home: &Path) -> Vec<PathBuf> {
    if cfg!(windows) {
        windows_cli_dirs(home)
    } else {
        unix_cli_dirs(home)
    }
}

fn windows_cli_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![
        home.join(".local").join("bin"),
        home.join("AppData").join("Roaming").join("npm"),
        home.join("AppData").join("Local").join("Volta").join("bin"),
    ];
    if let Ok(appdata) = env::var("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("npm"));
    }
    if let Ok(local) = env::var("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("Volta").join("bin"));
    }
    dirs.push(PathBuf::from(r"C:\nvm4w\nodejs"));
    dirs
}

fn unix_cli_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![
        home.join(".local").join("bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    dirs.extend(nvm_node_bins(home));
    dirs.push(home.join(".volta").join("bin"));
    dirs.push(home.join(".bun").join("bin"));
    dirs.push(home.join(".npm-global").join("bin"));
    if cfg!(target_os = "macos") {
        dirs.push(
            home.join("Library")
                .join("Application Support")
                .join("fnm")
                .join("aliases")
                .join("default")
                .join("bin"),
        );
        dirs.push(home.join("Library").join("pnpm"));
    }
    dirs.push(
        home.join(".local")
            .join("share")
            .join("fnm")
            .join("aliases")
            .join("default")
            .join("bin"),
    );
    dirs.push(home.join(".local").join("share").join("pnpm"));
    dirs
}

/// `~/.nvm/versions/node/<version>/bin`, newest version first.
fn nvm_node_bins(home: &Path) -> Vec<PathBuf> {
    let root = home.join(".nvm").join("versions").join("node");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut versions: Vec<(Vec<u64>, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join("bin").is_dir())
        .map(|path| (node_version_key(&path), path))
        .collect();
    versions.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    versions
        .into_iter()
        .map(|(_, path)| path.join("bin"))
        .collect()
}

fn node_version_key(path: &Path) -> Vec<u64> {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .trim_start_matches('v')
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// PATH entries as the user's login shell sees them, probed once per process.
///
/// Empty on Windows and in tests (which inject directories explicitly instead).
pub fn login_shell_path_dirs() -> &'static [PathBuf] {
    static DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    DIRS.get_or_init(|| {
        if cfg!(windows) || cfg!(test) {
            return Vec::new();
        }
        probe_login_shell_path(&login_shell(), SHELL_PROBE_TIMEOUT).unwrap_or_default()
    })
}

fn login_shell() -> PathBuf {
    login_shell_from(env::var_os("SHELL"))
}

/// `$SHELL` when it points at a real file, else the platform's default shell.
fn login_shell_from(shell: Option<OsString>) -> PathBuf {
    shell
        .map(PathBuf::from)
        .filter(|shell| shell.is_file())
        .unwrap_or_else(|| {
            PathBuf::from(if cfg!(target_os = "macos") {
                "/bin/zsh"
            } else {
                "/bin/sh"
            })
        })
}

/// Run `shell -i -l -c` to dump the environment between markers and pull PATH out of it.
///
/// `-i -l` mirrors a terminal (login + interactive), which is where node version managers
/// register themselves. Output is read via `/usr/bin/env` so the result does not depend on
/// how the shell expands `$PATH`.
fn probe_login_shell_path(shell: &Path, timeout: Duration) -> Option<Vec<PathBuf>> {
    let script = format!("printf '\\n{ENV_START}\\n'; /usr/bin/env; printf '{ENV_END}\\n'");
    let child = Command::new(shell)
        .args(["-i", "-l", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    match wait_with_deadline(child, timeout).ok()? {
        CommandOutcome::Exited { stdout, .. } => parse_login_shell_path(&stdout),
        CommandOutcome::TimedOut => None,
    }
}

fn parse_login_shell_path(output: &str) -> Option<Vec<PathBuf>> {
    let start = output.find(ENV_START)? + ENV_START.len();
    let end = output[start..].find(ENV_END)? + start;
    let path_line = output[start..end]
        .lines()
        .find_map(|line| line.strip_prefix("PATH="))?;
    let dirs: Vec<PathBuf> = env::split_paths(path_line)
        .filter(|dir| !dir.as_os_str().is_empty())
        .collect();
    (!dirs.is_empty()).then_some(dirs)
}

pub fn agent_info(id: AgentId, binary: &str) -> AgentInfo {
    let cli_ok = resolve_cli_binary(binary).is_some();
    AgentInfo {
        id,
        display_name: id.display_name().to_string(),
        cli_ok,
        cli_error: if cli_ok {
            None
        } else {
            Some(format!("{} CLI not found.", id.display_name()))
        },
        install_git: cli_ok,
        install_folder: cli_ok,
        plugin_toggle: match id {
            AgentId::Claude => cli_ok,
            AgentId::Codex => true,
            AgentId::Antigravity => cli_ok,
            AgentId::Cursor => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::cli_stub::CliStub;
    use crate::paths::scratch_dir;

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
}
