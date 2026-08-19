# Prerelease smoke test (Windows and macOS)

Early prerelease build of **on-n-off**: desktop switchboard for Claude / Codex / Antigravity / Cursor plugins, skills, MCP, and local usage estimates.

The Windows notes come first; macOS-specific notes are in [macOS](#macos) below. The smoke order and safety notes apply to both.

## What’s new (Studio v2 chrome)

- App mark / window icons refreshed (rocker-style circular glyph)
- Theme control moved from the top strip into left-rail **Appearance** (Dark / Light)
- Left rail icons + hover well; Usage kept as a dedicated screen and Overview summary
- Overview gauges link to Plugins / Skills / MCP; Usage card links to full Usage

## What you get

- Windows installer: `on-n-off_0.1.0_x64-setup.exe` (NSIS)
- Not Authenticode-signed → Windows SmartScreen may warn; choose “More info” → “Run anyway” if you trust the sender
- Stable releases include mandatory Tauri updater signatures for in-app update integrity

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

1. Run the setup exe
2. Launch **on-n-off**
3. Confirm agent tabs show CLI status (green = found on PATH)
4. Open **Settings → Application updates** and confirm the installed version and installer format are correct

## Suggested smoke order

1. **Overview / Plugins / Skills / MCP** — browse only first
2. **Usage** — scan local transcripts (read-only of agent files; may write caches under `%USERPROFILE%\.on-n-off\`)
3. **Limits** — live Claude / Codex subscription limits via the CLIs' stored logins (never writes to or refreshes them; outbound HTTPS to `api.anthropic.com` and `chatgpt.com`; on macOS, click **Allow** on the one-time Keychain prompt for `security`). Remembers each account's last numbers under `%USERPROFILE%\.on-n-off\limits\` so switching accounts with `codex login` / `claude` keeps the other one listed; **Forget** on a stale card deletes just that file.
4. **Agent config** — check paths and project scope
5. Only then try a **single toggle** or install on a throwaway plugin if you want write coverage

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

Ordinary local release build (no signed updater artifacts):

```bash
bun run tauri build
```

The installer lands under `src-tauri/target/release/bundle/nsis/` (or your `CARGO_TARGET_DIR` if set).

The GitHub release workflow builds the NSIS installer with the production
updater configuration and signing secrets. Do not publish its draft until the
installer, its `.sig` file, `latest.json`, checksums, and attestations have been
inspected.

## macOS

### What you get

- Disk image for Apple Silicon: `on-n-off_<version>_aarch64.dmg` (drag `on-n-off.app` to Applications)
- The app is ad-hoc signed and **not notarized** → Gatekeeper blocks the first launch. Right-click `on-n-off.app` → **Open**, or allow it under **System Settings → Privacy & Security**. Verify the SHA-256 checksum or GitHub attestation first.
- Stable releases ship a signed `on-n-off_<version>_aarch64.app.tar.gz` for in-app updates (`darwin-aarch64`)

### Prerequisites

1. macOS on Apple Silicon (Intel Macs are not built yet)
2. Agent CLIs installed any usual way (`claude` native installer, `npm`/`nvm`/`volta`/`bun` for `codex`, Homebrew…). Finder launches apps with a minimal `PATH`; the app asks your login shell (`$SHELL -i -l`) for its `PATH` once at startup and also checks well-known folders (`~/.local/bin`, `/opt/homebrew/bin`, `~/.nvm/versions/node/*/bin`, `~/.volta/bin`, `~/.bun/bin`, …). If a CLI still shows missing, run `which <cli>` in Terminal and paste that path into **Settings → Binary**.
3. Optional: Node/`npx` for `npx skills add …`

The app reads `~/.claude`, `~/.codex`, `~/.gemini`, `~/.cursor`, and `~/.on-n-off`. Because it inspects project folders (for project-scoped MCP/skills and transcripts), macOS may ask for access to **Documents** or **Desktop** the first time; declining only hides those project-scoped entries.

### From source on macOS

```sh
xcode-select --install          # once
curl https://sh.rustup.rs -sSf | sh   # once
bun install
bun run test
bun run tauri dev
bun run tauri build --bundles app,dmg   # .app + .dmg under src-tauri/target/release/bundle/
```

The `.dmg` step styles the Finder window through AppleScript, so a local build needs Automation permission for your terminal (System Settings → Privacy & Security → Automation → Finder). Without it the `.app` is still produced under `src-tauri/target/release/bundle/macos/`; the release workflow builds the `.dmg` on GitHub's macOS runners.

## Out of scope for this smoke

- WSL-only agent installs
- Signed/notarized installers (Windows Authenticode, Apple notarization)
- Intel (x86_64) macOS builds
- Project-level write/disable
- Antigravity usage transcripts
- Subscription billing accuracy (Usage is API-equivalent estimate; Limits shows the vendors' own percentages)

The product is still taking shape. Open a GitHub issue with reproducible steps if anything breaks on your system. Do not include agent transcripts, credentials, or private configuration in an issue.
