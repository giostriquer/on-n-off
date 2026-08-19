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
Release publication remains manual; the tag workflow prepares a draft with the
Windows installers, the macOS disk image and updater bundle, signatures,
checksums, attestations, and `latest.json`.

## Acknowledgements

The Usage analytics implementation is derived in part from
[T3 Code](https://github.com/pingdotgg/t3code), Copyright (c) 2026 T3 Tools Inc.
See [THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md) for the exact source
snapshot and MIT license notice.

## License

on-n-off is available under the [MIT License](./LICENSE).
