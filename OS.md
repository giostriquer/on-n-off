# OS.md — platform differences that bite this app

Working notes on Windows vs macOS (and Linux where it applies). Everything here was hit in
practice while building on-n-off; keep it factual and add the date or PR when you learn something new.

## Where a GUI app's PATH comes from

| | Windows | macOS |
|---|---|---|
| Launched from a terminal | inherits the terminal's PATH | inherits the terminal's PATH |
| Launched from the shell/Finder | the PATH the parent (Explorer) had **when it started**; installers that append to the registry PATH are invisible until Explorer or the machine restarts | a **minimal** PATH (`/usr/bin:/bin:/usr/sbin:/sbin`); nothing from `~/.zshrc`, so nvm/volta/homebrew shims are invisible |
| Fix in `cli_locate.rs` | second tier = the registered user + machine PATH from `HKCU\Environment` and `HKLM\...\Session Manager\Environment` (`%VAR%` expanded, HKLM values are `REG_EXPAND_SZ`) | second tier = `$SHELL -i -l -c` probed once, off the UI thread, 5 s cap |
| Third tier | `~/.local/bin`, `%APPDATA%\npm`, `%LOCALAPPDATA%\Volta\bin`, `%LOCALAPPDATA%\agy\bin`, `%LOCALAPPDATA%\cursor-agent`, `%LOCALAPPDATA%\Programs\GitHub CLI`, `%ProgramFiles%\GitHub CLI`, `C:\nvm4w\nodejs` | `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`, nvm versions (newest first), volta, bun, npm-global, fnm, pnpm |

Spawned CLIs get the merged list as their `PATH` so `#!/usr/bin/env node` shims work.

The GitHub CLI (`gh`, used by the Pull requests screen) is found the same way; its Windows roots are the MSI / machine-scope winget folder under `%ProgramFiles%` and the user-scope winget folder under `%LOCALAPPDATA%\Programs` (added 2026-08 with the Pull requests screen). On macOS `gh auth token` reads `gh`'s own Keychain item; **observed** (2026-08, release `.app` launched with `open`): the read completes without a Keychain prompt for on-n-off, presumably because the item's ACL names `gh`'s helper rather than the calling app.

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

## Fonts

- The UI uses the platform's own type: `-apple-system`/SF Pro Text and SF Mono on macOS, Segoe UI and
  Cascadia Mono (Consolas before Windows 11) on Windows, with `system-ui` and the generic families as
  the last fallbacks (`ui/src/tokens.css`, the `--font-sans`/`--font-mono` tokens; every stylesheet
  reads those). No webfont is bundled: the previous Instrument Sans / JetBrains Mono rendered fuzzy at
  the screens' 10–12 px sizes on 1× displays, where native faces are hinted. `-webkit-font-smoothing`
  is left at its default for the same reason (macOS only; WebView2 ignores it).

## PowerShell vs bash in this repo

- CI and `scripts/*.ps1` run under PowerShell 7 on both runners; scripts must stay path-neutral.
- CI Rust steps cost far more on the Windows runner than on macOS (4 cores against Apple Silicon,
  plus Defender scanning every object file cargo writes). The Windows jobs exclude the workspace,
  `~/.cargo`, and `~/.rustup` from Defender with `Add-MpPreference`; the step is `continue-on-error`
  because it is a speed measure, not a correctness one, and it is runner-only — never run it on a
  development machine. Platform-neutral frontend checks (`bun run test`, `bun run check`) run once
  on `ubuntu-latest` instead of on both native legs; `bun run build` stays native because
  `tauri-build` needs `ui/dist` before the Rust steps.
- In this repo's shell tooling, prefer `Join-Path`, `$env:VAR`, `-LiteralPath`; in bash use forward
  slashes and `cygpath -w` when handing paths to Windows programs.

## Native side notch: macOS

The opt-in side notch is a SwiftUI/AppKit helper bundled under `Contents/Helpers/on-n-off-notch.app`
with its own native application identity. `src-tauri/native_build.rs` compiles it only for macOS;
`tauri.macos.conf.json` adds the binary to the macOS bundle. Swift Command Line
Tools suffice: native model checks use an executable test target, not XCTest.
The helper has no provider credentials, network listener, or independent login
item. Rust supervises its bounded stdin/stdout protocol and reaps it on shutdown.
A closed parent pipe makes the helper exit, including after a parent crash.
Display UUIDs retain the existing settings format. Screen placement converts
Core Graphics top-left coordinates into AppKit bottom-left coordinates. The helper owns three
non-activating panels (hover pill, rail, popover); hovering never activates it, and only a pinned
popover takes key status so Escape can release it. Live-session rows come from
`~/.claude/sessions/<pid>.json` (the pid is checked with `/bin/ps`) and from Codex rollouts modified
in the last hour; the helper never opens those files itself.

## Native side notch: Windows

Windows has no helper process. One transparent, always-on-top, non-activating `tao` window per
selected display is painted with tiny-skia and presented through `UpdateLayeredWindow`, which
carries the new origin, size and pixels in one call. These behaviours decide whether it works:

- **The window must be a bare `WS_POPUP`.** tao leaves an undecorated window with `WS_CAPTION` and
  `WS_THICKFRAME`; Windows then keeps an invisible resize border on the left, right and bottom, so
  the client area is inset by the border width, the layered surface is clipped to it, and the
  compositor draws a drop shadow and a top hairline around what is left. `overlay_style` strips the
  frame before every present, which is also what lets the rail reach the screen edge.
- **Hit testing is per-pixel alpha.** A transparent part of the overlay clicks through to whatever is
  underneath, so the rail's flared ends and the gap beside a popover are not clickable, and
  `WindowFromPoint` is what tells the state machine whether the pointer is on the notch.
- **tao reports no `CursorMoved` for this window.** The 80 ms pointer poll is the only hover input
  there is, and it has to keep running while the collapsed strip is showing or reaching the strip
  never opens the rail.
- **The text engine hints every pixel size on its own.** A string measured at the point size is not `scale` times
  narrower than the same string rasterised at point x scale, so measurement for layout has to happen
  at the device size and convert back, or a 150 % display drifts by about a fifth on longer labels.
- **The rail has to sit on whole device pixels.** Centring it in a work area that does
  not divide evenly (1032 px tall, say) leaves it on a half pixel, and the window origin
  is the union of the rail with the popover: opening a popover above the rail's top edge
  then shifts that half pixel into the rail's own drawing and every cell visibly jumps.
  `model::layout` snaps the frame with `pixel_aligned`, the `pixelAligned` port.
- **A non-activating overlay cannot take the foreground.** The popover's "Open Limits" and "Open Pull
  requests" links show and raise the main window, but Windows refuses the focus change, so a window
  that was already open behind another app stays behind it and the taskbar button flashes instead.

- **Text goes through DirectWrite, along the WebView's own path.** GDI's
  `ANTIALIASED_QUALITY` is a 4x4 supersample — exactly 16 coverage levels, and curves that
  stair-step beside the app's WebView text. `win_paint/text.rs` shapes each run with
  `IDWriteFontFace` and rasterises it through `IDWriteGlyphRunAnalysis` in
  `CLEARTYPE_NATURAL_SYMMETRIC`, takes the **3x1 ClearType texture**, and collapses each
  triple by sRGB luminance — what Skia does when Chromium needs a grayscale alpha mask, and
  softer at the stems than DirectWrite's own 1x1 grayscale texture, which comes out a pixel
  narrower per word and harder at the edges. A layered overlay composites per-pixel alpha
  and cannot show subpixel colour; that is the only part of the path it cannot follow.
- **The alpha texture is linear coverage.** Every Windows text stack corrects it for the
  display before blending, and skipping that leaves light text on a dark panel a visible
  step thinner than the same string in the app. The correction is the sRGB display exponent,
  *not* the system's ClearType gamma (`IDWriteRenderingParams::GetGamma`, 1.8 on a default
  install), which lands far too light.
- **The face is the app's own, and so is the weight it resolves to.** `ui/src/tokens.css`
  asks for `-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", system-ui,
  sans-serif`; on Windows the first three do not resolve, so the WebView draws in `Segoe UI`
  and the notch asks for the same family (`Tahoma` only as a last resort). That family ships
  300/350/400/600/700 and no 500. CSS would step a 500 request *down* onto regular, but
  Chromium hands the choice to DirectWrite, which steps it *up* onto semibold — so the app's
  own `font-medium` runs render semibold. `installed_face` asks DirectWrite the same
  question, through `GetFirstMatchingFont`; scoring the family by hand instead put every
  percentage on the rail a full weight lighter than the same figure in the app.
- **Glyphs beside text line up on the cap band.** SF Pro's line box happens to sit on its
  cap centre, so the mac design's `HStack` and the app's `flex items-center` both land there.
  Segoe UI's ascent runs far above its cap line and its descender far below, so neither the
  line box nor the whole ink extent lands where the eye expects — centring on the ink drops a
  header mark about a pixel and a half below its title. `text::cap_middle_px` gives the cap
  offset from the real font metrics.
- **Typography is checked against the app engine, not by eye.**
  `win_paint::visual::specimen` renders every size and weight the notch draws on one sheet.
  Rebuild the same rows as HTML and render them with `msedge --headless`, once plain and
  once with `--disable-lcd-text` — the WebView sits between those two. Advance widths and
  ink centroids should match outright and total ink to a few per cent. Measure
  alpha-weighted: a premultiplied PNG read as RGB reports every partial pixel as solid.

Win11 is the floor (`CurrentBuildNumber` >= 22000); the settings card says so on older builds.

