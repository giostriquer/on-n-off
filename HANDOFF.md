# Coworker handoff — Windows smoke test

Private early build of **on-n-off**: desktop switchboard for Claude / Codex / Antigravity plugins, skills, MCP, and local usage estimates.

## What’s new (Studio v2 chrome)

- App mark / window icons refreshed (rocker-style circular glyph)
- Theme control moved from the top strip into left-rail **Appearance** (Dark / Light)
- Left rail icons + hover well; Usage kept as a dedicated screen and Overview summary
- Overview gauges link to Plugins / Skills / MCP; Usage card links to full Usage

## What you get

- Windows installer: `on-n-off_0.1.0_x64-setup.exe` (NSIS), or the MSI next to it
- Unsigned → Windows SmartScreen may warn; choose “More info” → “Run anyway” if you trust the sender

## Prerequisites (Windows, not WSL)

1. **WebView2** — usually already on Windows 10/11
2. Agent CLIs on the **Windows** PATH (so the app can see them):
   - `claude`
   - `codex`
   - `agy` (Antigravity; optional)
3. Optional: Node/`npx` if you will install skills via `npx skills add …`

This is a **Windows desktop** app. It reads `%USERPROFILE%\.claude`, `.codex`, `.gemini`, and `.on-n-off`.  
CLIs or transcripts that exist **only inside WSL** will look missing unless the same tools/data also exist on the Windows side.

If a CLI works in a terminal but the app says **os error 193** / “not a valid Win32 application”, open **Settings**, click **Diagnose**, and point **Binary** at the `.cmd` or `.exe` next to the nvm/npm shim (the extensionless `claude` file is not a Windows program).

## Install & run

1. Run the setup exe (or MSI)
2. Launch **on-n-off**
3. Confirm agent tabs show CLI status (green = found on PATH)

## Suggested smoke order

1. **Overview / Plugins / Skills / MCP** — browse only first
2. **Usage** — scan local transcripts (read-only of agent files; may write caches under `%USERPROFILE%\.on-n-off\`)
3. **Agent config** — check paths and project scope
4. Only then try a **single toggle** or install on a throwaway plugin if you want write coverage

## Safety notes

- File-patch toggles backup under `%USERPROFILE%\.on-n-off\backups` and roll back on write failure
- CLI-driven install/enable/uninstall backs up config first, but does **not** undo CLI side effects automatically
- `npx skills` installs are not backed up by on-n-off
- Prefer not to run “master cut” (off by default) on a real machine

## From source (alternative)

```bash
bun install
bun run test
bun run tauri dev
```

Needs Bun + Rust toolchain + WebView2.

Release build:

```bash
bun run tauri build
```

Installers land under `src-tauri/target/release/bundle/nsis/` and `…/msi/` (or your `CARGO_TARGET_DIR` if set).

## Out of scope for this smoke

- WSL-only agent installs
- Signed/notarized installer
- Project-level write/disable
- Antigravity usage transcripts
- Subscription billing accuracy (Usage is API-equivalent estimate)

Private while the product is taking shape — ping the sender with anything that breaks on your box.
