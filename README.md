# on-n-off

A desktop switchboard for Claude, Codex, Antigravity, and Cursor plugins,
skills, MCP servers, usage estimates, and agent configuration, for Windows and
macOS (Apple Silicon). The agent CLIs and their global configuration remain the
source of truth.

The frontend uses React, TypeScript, Vite, Tailwind, and TanStack Router/Query.
The desktop backend uses Tauri 2 and Rust.

## Current status

on-n-off is an early prerelease. It can inspect real agent homes and can apply
configuration changes, so review the confirmation prompt before each write.
Configuration writes use guarded replacement, validation, backups, and rollback.
CLI-driven installs and uninstalls can have side effects outside that rollback
boundary.

See [HANDOFF.md](./HANDOFF.md) for per-platform prerequisites and the
recommended smoke-test order.

### macOS side notch

The notch is a native SwiftUI view in an AppKit panel. A small helper is bundled
inside the macOS app and runs while the feature is enabled. It closes with the
parent app; there is no extra installation, login item, or local network service.
The main application remains Tauri.

In **Settings → Side notch**, choose a display, a Compact, Standard, or Large
size, and its left or right edge, then enable the notch. Existing settings use
Standard until another size is selected. It shows Claude and Codex usage for
the current accounts; click a ring for quota windows and reset times, or the arc
below for settings.
Claude's outer ring shows total weekly usage; its lighter orange inner ring shows
Fable's weekly usage when available. Five-hour usage remains in the details.
Escape or clicking outside collapses the details. The menu-bar tray stays available.

The selection is saved by display UUID in `~/.on-n-off/side-notch.json`. The
notch hides when that display disconnects or joins a mirror set, and returns when
it is available again. It never moves to another display automatically. It is an
overlay, so choose the edge opposite your Dock. It does not reserve screen space.
Background usage refreshes every minute without forcing another Keychain read.

## Run from source

Install [Bun](https://bun.sh/), the Rust toolchain, and the Tauri prerequisites
for your platform (WebView2 on Windows; Xcode Command Line Tools on macOS). The
commands are the same in PowerShell and in a macOS shell:

```sh
bun install
bun run test
bun run check
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features
bun run tauri dev
```

## Release builds

Windows (NSIS installer under `src-tauri/target/release/bundle/nsis/`):

```powershell
bun run tauri build
```

macOS (an ad-hoc-signed `on-n-off.app` under `bundle/macos/` and a `.dmg` under
`bundle/dmg/`):

```sh
bun run tauri build --bundles app,dmg
```

CI drives both platforms through `scripts/build-bundle.ps1 -InstallerKind nsis|dmg`,
which also validates and stages the release assets; it needs PowerShell 7 locally.

Ordinary local builds are unsigned: Windows can show a SmartScreen warning, and
macOS Gatekeeper blocks the first launch until you right-click the app and choose
Open (or allow it under System Settings → Privacy & Security). The macOS build is
not notarized.

## Application updates

Published stable releases provide signed update paths for the NSIS installer on
Windows and for the Apple Silicon app bundle on macOS
(`darwin-aarch64`, delivered as `.app.tar.gz`). After the initially selected
provider is ready, an installed release checks the stable GitHub Releases feed. Automatic download is enabled by
default and can be disabled in Settings. Installation and restart always require
an explicit user action.

Tauri updater signatures verify release integrity but are not Windows
Authenticode signatures or Apple notarization. Windows can still show an
unknown-publisher warning and macOS can still show a Gatekeeper warning.
Release publication remains manual; the tag workflow waits for approval of the
`release` environment (which holds the updater signing key), then prepares a
draft with the Windows installer, the macOS disk image and updater bundle,
signatures, checksums, attestations, and `latest.json`.

## Acknowledgements

The Usage analytics implementation is derived in part from
[T3 Code](https://github.com/pingdotgg/t3code), Copyright (c) 2026 T3 Tools Inc.
See [THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md) for the exact source
snapshot and MIT license notice.

## License

on-n-off is available under the [MIT License](./LICENSE).

### Native notch development (macOS)

The Rust build compiles and stages the Swift helper automatically with Xcode
Command Line Tools (`xcrun swift`). Windows does not build or bundle this helper.
The helper uses macOS 11-compatible APIs, explicit main-actor UI ownership, and
sendable value snapshots. Complete concurrency checks are enabled for every Swift
target, and the Rust build treats Swift warnings as errors. To run its model and protocol/lifetime
checks after a Rust build:

```sh
xcrun swift run --package-path src-tauri/macos/SideNotch -Xswiftc -warnings-as-errors NotchCoreChecks
bun scripts/check-native-notch.mjs src-tauri/target/debug/on-n-off-notch
```

Native visuals must be inspected in the running app. The browser screenshot
harness covers the main settings page but cannot render the SwiftUI panel.
