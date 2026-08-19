# OS.md — platform differences that bite this app

Working notes on Windows vs macOS (and Linux where it applies). Everything here was hit in
practice while building on-n-off; keep it factual and add the date or PR when you learn something new.

## Where a GUI app's PATH comes from

| | Windows | macOS |
|---|---|---|
| Launched from a terminal | inherits the terminal's PATH | inherits the terminal's PATH |
| Launched from the shell/Finder | the PATH the parent (Explorer) had **when it started**; installers that append to the registry PATH are invisible until Explorer or the machine restarts | a **minimal** PATH (`/usr/bin:/bin:/usr/sbin:/sbin`); nothing from `~/.zshrc`, so nvm/volta/homebrew shims are invisible |
| Fix in `cli_locate.rs` | second tier = the registered user + machine PATH from `HKCU\Environment` and `HKLM\...\Session Manager\Environment` (`%VAR%` expanded, HKLM values are `REG_EXPAND_SZ`) | second tier = `$SHELL -i -l -c` probed once, off the UI thread, 5 s cap |
| Third tier | `~/.local/bin`, `%APPDATA%\npm`, `%LOCALAPPDATA%\Volta\bin`, `%LOCALAPPDATA%\agy\bin`, `%LOCALAPPDATA%\cursor-agent`, `C:\nvm4w\nodejs` | `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`, nvm versions (newest first), volta, bun, npm-global, fnm, pnpm |

Spawned CLIs get the merged list as their `PATH` so `#!/usr/bin/env node` shims work.

## Launchers and executables

- Windows: npm/nvm shims are extensionless files that Windows cannot spawn (os error 193). Prefer the
  `.cmd` / `.exe` next to them (`PATHEXT`, default `.EXE;.CMD;.BAT;.COM`). `launchable()` returns the
  file with PATHEXT's casing (`agent.CMD`), so compare launcher paths case-insensitively in tests.
- Unix: a candidate must carry the executable bit; a symlink counts (`fs::canonicalize` to see where
  it really lives — Cursor's `~/.local/bin/agent` resolves into `~/.local/share/cursor-agent/...`).
- Test doubles: `cli_stub.rs` writes a `.cmd` on Windows and an executable `sh` script elsewhere.
  Never hand-write batch files in tests.
- Child processes: drain stdout and stderr concurrently from spawn (`process.rs`); waiting for exit
  first deadlocks on full pipes on both platforms.

## Filesystem

- Paths: no hard-coded drive letters or `\`; build with `Path::join`. Windows paths compare
  case-insensitively — dedupe with a lower-cased key on Windows only.
- `File::set_modified` on a **directory** handle fails on Windows (needs backup semantics). Order
  versioned folders by manifest content / file mtimes, not directory mtimes.
- Symlinks: creating them on Windows needs Developer Mode or admin, and Git checks a symlink out as a
  plain text file unless `core.symlinks=true`. Do not rely on repo symlinks (this is why `CLAUDE.md`
  is `@AGENTS.md`, not a link).
- Line endings: `.gitattributes` normalises to LF; `.ps1/.cmd/.bat` stay CRLF. Write files from
  scripts with explicit `\n`.
- Temp dirs: `%LOCALAPPDATA%\Temp` vs `/tmp`; use `paths::scratch_dir` in tests and clean up.

## Processes, ports, sandboxes

- Vite dev port `1420` is `strictPort`; a second app/worktree needs `--port` (Vite CLI flag beats
  config) plus a Tauri `--config` override of `build.devUrl` / `beforeDevCommand`.
- WebView2 (Windows) shares one browser process per user-data folder. Two on-n-off builds with the
  same identifier share it, so `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` (e.g.
  `--remote-debugging-port=9222`) only applies if you also set a private `WEBVIEW2_USER_DATA_FOLDER`.
  With that, the app can be driven over CDP for read-only QA.
- Restricted Windows sandboxes: Vite/esbuild can fail with `spawn EPERM`; rerun in an approved
  context, do not patch code around it.
- macOS: the built `.app` launched with `open` has the Finder PATH — test CLI resolution that way.
  Reading agent homes may prompt for Documents/Desktop access; `Limits` triggers a one-time Keychain
  prompt for `/usr/bin/security`. Limit notifications use the native notification permission;
  the monitor detects sleep/wake from a wall-clock heartbeat because desktop Tauri does not emit
  `RunEvent::Resumed`.
- Windows: native notifications must be tested from an installed build; a development build can use
  PowerShell's app name and icon instead of on-n-off's identity.

## Packaging and updates

- Windows: NSIS only (`ON_N_OFF_INSTALLER_KIND=nsis` is a `rerun-if-env-changed` build input, so each
  installer kind is its own `tauri build` pass); unsigned → SmartScreen warning; verify SHA-256 / attestation.
- macOS: `.app` + `.dmg` (Apple Silicon), ad-hoc signed, not notarised → right-click → Open on first
  launch. The dmg step needs Automation permission for the terminal locally.
- Icons: `icon.icns` carries macOS margins + drop shadow; Windows `icon.ico` / PNGs must be full-bleed
  with transparent corners (no shadow). Regenerate the Windows set from the icns master, not the
  other way round.
- Windows taskbar icon: Windows 11 draws a running window's taskbar button from the shell's cached
  icon for the executable path, not from the window's `WM_SETICON` icon, so an in-place upgrade can keep
  showing the previous release's icon. `src-tauri/windows/hooks.nsh` (`NSIS_HOOK_POSTINSTALL`) sends
  `SHChangeNotify(SHCNE_ASSOCCHANGED)` after install to drop the cached icon; on a machine that still
  shows a stale one, run `ie4uinit.exe -show` and relaunch the app (or restart Explorer).

## PowerShell vs bash in this repo

- CI and `scripts/*.ps1` run under PowerShell 7 on both runners; scripts must stay path-neutral.
- In this repo's shell tooling, prefer `Join-Path`, `$env:VAR`, `-LiteralPath`; in bash use forward
  slashes and `cygpath -w` when handing paths to Windows programs.
