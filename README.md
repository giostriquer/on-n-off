# on-n-off

A Windows-first desktop switchboard for Claude, Codex, and Antigravity plugins,
skills, MCP servers, usage estimates, and agent configuration. The agent CLIs and
their global configuration remain the source of truth.

The frontend uses React, TypeScript, Vite, Tailwind, and TanStack Router/Query.
The desktop backend uses Tauri 2 and Rust.

## Current status

on-n-off is an early prerelease. It can inspect real agent homes and can apply
configuration changes, so review the confirmation prompt before each write.
Configuration writes use guarded replacement, validation, backups, and rollback.
CLI-driven installs and uninstalls can have side effects outside that rollback
boundary.

See [HANDOFF.md](./HANDOFF.md) for Windows prerequisites and the recommended
smoke-test order.

## Run from source

Install [Bun](https://bun.sh/), the Rust toolchain, and the Tauri prerequisites
for Windows. Then run:

```powershell
bun install
bun run test
bun run check
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features
bun run tauri dev
```

## Windows release build

```powershell
bun run tauri build
```

NSIS and MSI installers are written under `src-tauri/target/release/bundle/`.
Ordinary local builds are unsigned and can trigger a Windows SmartScreen warning.

## Application updates

Published stable releases provide separate signed update paths for NSIS and MSI
installations. After the initially selected provider is ready, an installed
release checks the stable GitHub Releases feed. Automatic download is enabled by
default and can be disabled in Settings. Installation and restart always require
an explicit user action.

Tauri updater signatures verify release integrity but are not Windows
Authenticode signatures. Windows can still show an unknown-publisher warning.
Release publication remains manual; the tag workflow prepares a draft with both
installers, signatures, checksums, attestations, and `latest.json`.

## Acknowledgements

The Usage analytics implementation is derived in part from
[T3 Code](https://github.com/pingdotgg/t3code), Copyright (c) 2026 T3 Tools Inc.
See [THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md) for the exact source
snapshot and MIT license notice.

## License

on-n-off is available under the [MIT License](./LICENSE).
