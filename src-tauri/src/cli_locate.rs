//! Where agent CLIs live.
//!
//! GUI apps do not always inherit the PATH a terminal has: on macOS, Finder launches with a
//! minimal one, so nvm/volta/homebrew shims are invisible; on Windows, an app started before an
//! installer appended to the registry PATH never sees the new folder. The app therefore
//! searches one merged list — the process PATH, then the login shell's PATH (probed once) or the
//! registered user + machine PATH, then well-known install folders — and hands that same list
//! to spawned CLIs as their PATH, so `#!/usr/bin/env node` launchers keep working wherever they
//! were found.
//!
//! Resolution is per provider: Cursor's CLI is called `agent`, a name other products use too,
//! so [`resolve_provider_cli`] only accepts an `agent` that provably belongs to Cursor.

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
            environment_path_dirs(),
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
    environment_dirs: &[PathBuf],
    well_known: Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut merged: Vec<PathBuf> = Vec::new();
    let process_dirs = process_path
        .map(|value| env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_default();
    for dir in process_dirs
        .into_iter()
        .chain(environment_dirs.iter().cloned())
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

/// [`resolve_cli_binary`] for a specific provider: an explicit path or settings override always
/// wins; otherwise Cursor is located with [`find_cursor_cli`] and every other provider by name.
pub fn resolve_provider_cli(id: AgentId, name: &str) -> Option<PathBuf> {
    resolve_provider_cli_in(id, name, cli_search_path())
}

fn resolve_provider_cli_in(id: AgentId, name: &str, search_path: &[PathBuf]) -> Option<PathBuf> {
    let candidate = PathBuf::from(name);
    if let Some(path) = launchable(&candidate) {
        return Some(path);
    }
    if let Some(override_path) = crate::settings::binary_override_for(name) {
        if let Some(path) = launchable(&override_path) {
            return Some(path);
        }
    }
    match id {
        AgentId::Cursor => find_cursor_cli(search_path),
        _ => find_in_dirs(name, search_path),
    }
}

const CURSOR_INSTALL_DIR: &str = "cursor-agent";

/// The Cursor CLI: `agent` (canonical) or `cursor-agent` (legacy alias), first match in
/// `dirs` order — but an `agent` only counts when it provably belongs to Cursor, since other
/// products (Grok, ...) install a launcher of the same name onto PATH.
///
/// Cursor's installers keep everything under a `cursor-agent` folder: `%LOCALAPPDATA%\cursor-agent`
/// on Windows, `~/.local/share/cursor-agent/versions/<v>/` behind a `~/.local/bin/agent` symlink
/// elsewhere. So an `agent` qualifies when that folder name appears in its path or in the path
/// the symlink resolves to.
pub(crate) fn find_cursor_cli(dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter().find_map(|dir| {
        launchable(&dir.join("agent"))
            .filter(|path| belongs_to_cursor(path))
            .or_else(|| launchable(&dir.join(CURSOR_INSTALL_DIR)))
    })
}

fn belongs_to_cursor(launcher: &Path) -> bool {
    let has_install_dir = |path: &Path| {
        path.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(CURSOR_INSTALL_DIR))
        })
    };
    has_install_dir(launcher)
        || std::fs::canonicalize(launcher).is_ok_and(|real| has_install_dir(&real))
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
    windows_cli_dirs_from(home, &|name| env::var_os(name))
}

/// `lookup` stands in for the environment so tests can describe any machine.
fn windows_cli_dirs_from(home: &Path, lookup: &dyn Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut dirs = vec![
        home.join(".local").join("bin"),
        home.join("AppData").join("Roaming").join("npm"),
        home.join("AppData").join("Local").join("Volta").join("bin"),
    ];
    if let Some(appdata) = lookup("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("npm"));
    }
    if let Some(local) = lookup("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        dirs.push(local.join("Volta").join("bin"));
        // Native installers: Antigravity (`agy`) and the Cursor CLI (`agent`).
        dirs.push(local.join("agy").join("bin"));
        dirs.push(local.join(CURSOR_INSTALL_DIR));
        // `winget install GitHub.cli` in user scope.
        dirs.push(local.join("Programs").join("GitHub CLI"));
    }
    if let Some(program_files) = lookup("ProgramFiles") {
        // The GitHub CLI MSI and machine-scope winget installs.
        dirs.push(PathBuf::from(program_files).join("GitHub CLI"));
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

/// The second search tier: what the user's environment adds beyond the process PATH.
///
/// Unix: the login shell's PATH ([`login_shell_path_dirs`]). Windows: the PATH registered for
/// the user and the machine ([`registered_path_dirs`]), which a running app misses when an
/// installer appended to it after the app (or its parent shell) started.
fn environment_path_dirs() -> &'static [PathBuf] {
    if cfg!(windows) {
        registered_path_dirs()
    } else {
        login_shell_path_dirs()
    }
}

/// PATH entries from the Windows registry (`HKCU\Environment` then the machine-wide
/// `Session Manager\Environment`), `%VAR%` tokens expanded, read once per process.
///
/// Empty off Windows and in tests (which inject directories explicitly instead).
pub fn registered_path_dirs() -> &'static [PathBuf] {
    static DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    DIRS.get_or_init(|| {
        if cfg!(test) {
            return Vec::new();
        }
        registered_path_dirs_from(&registered_path_values(), |name| env::var_os(name))
    })
}

#[cfg(windows)]
fn registered_path_values() -> Vec<OsString> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    [
        (HKEY_CURRENT_USER, r"Environment"),
        (
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
    ]
    .into_iter()
    .filter_map(|(hive, key)| {
        RegKey::predef(hive)
            .open_subkey(key)
            .ok()?
            .get_value::<String, _>("Path")
            .ok()
            .map(OsString::from)
    })
    .collect()
}

#[cfg(not(windows))]
fn registered_path_values() -> Vec<OsString> {
    Vec::new()
}

/// Split registry PATH values into directories, expanding `%NAME%` with `lookup`; entries
/// that reference an unknown variable are dropped rather than searched literally.
fn registered_path_dirs_from(
    values: &[OsString],
    lookup: impl Fn(&str) -> Option<OsString>,
) -> Vec<PathBuf> {
    values
        .iter()
        .flat_map(|value| env::split_paths(value).collect::<Vec<_>>())
        .filter(|dir| !dir.as_os_str().is_empty())
        .filter_map(|dir| expand_percent_vars(&dir.to_string_lossy(), &lookup))
        .map(PathBuf::from)
        .collect()
}

fn expand_percent_vars(text: &str, lookup: &impl Fn(&str) -> Option<OsString>) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            out.push_str(&rest[start..]);
            return Some(out);
        };
        let name = &after[..end];
        if name.is_empty() {
            out.push('%');
        } else {
            out.push_str(&lookup(name)?.to_string_lossy());
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
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

pub fn agent_info(id: AgentId) -> AgentInfo {
    let cli_ok = resolve_provider_cli(id, id.binary_name()).is_some();
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
}
