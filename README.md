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
size, and an edge (right, left, top, or bottom), then pick **Always show** or
**Show on hover**. On hover, a small pill at the screen edge opens the rail when
the pointer reaches it. A small pin at the rail's end flips between the two
without opening Settings, for when the rail sits over something you need. The rail is the notch silhouette with one cell per provider:
a ring around the provider's mark and the percentage beneath. Claude's outer
ring is its weekly limit, with Fable's weekly limit on a lighter inner ring;
Codex's ring is its current 5-hour window. Rings use the
provider's own accent and turn amber from 70 % and red from 90 %. Cursor and Antigravity show a dash until they expose subscription limits;
**Integrations** picks which providers get a cell.

A last cell shows your pull requests: the ring is split into one arc per pull
request, green when CI passes, red when it fails, yellow while it runs, grey
without checks, with the count beneath. Its popover lists the Pull
requests screen's lists you chose under **Integrations** (only **Mine** by
default): each row opens the pull request on GitHub, and its copy button puts
"review please: <title>" on the clipboard with the title linked, ready to paste
into Slack. Nothing is written to GitHub; the lists come from the same read the
Pull requests screen makes.

Hovering a provider cell opens a popover with every quota window and its reset
time, then the tool's live sessions: Claude Code sessions from `~/.claude/sessions` and
Codex sessions from the last hour of rollouts, each with where it runs, its
project, whether it is working or idle, and how long ago it was active. Clicking
a cell pins the popover; Escape or a click elsewhere releases it. The menu-bar
tray stays available.

The selection is saved by display UUID in `~/.on-n-off/side-notch.json`. The
notch hides when that display disconnects or joins a mirror set, and returns when
it is available again. It never moves to another display automatically. It is an
overlay, so choose the edge opposite your Dock. It does not reserve screen space.
Usage refreshes on the Limits interval without forcing another Keychain read;
live sessions refresh every ten seconds while the rail is visible.

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

The lifecycle script also renders the rail, the hover pill, and one popover per
provider from a fixture message into `.tmp/notch-render/<edge>/*.png` through the
helper's `--render <message.json> <out-dir>` mode; look at those captures after
any change to the rail or popover. The browser screenshot harness covers the
settings page but cannot render the SwiftUI panels, and hover, pinning, and edge
placement still need a look at the running app.
