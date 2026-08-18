# AGENTS.md

## Project map

on-n-off is a Tauri 2 desktop application for Windows and macOS (Apple Silicon).

- `ui/`: React 19, TypeScript, Vite, Tailwind, TanStack Router/Query/Charts.
- `src-tauri/src/commands.rs`: the IPC boundary used by `ui/src/lib/api.ts`.
- `src-tauri/src/{claude,codex,antigravity,cursor}.rs`: provider adapters.
- `src-tauri/src/paths.rs`: agent homes and app data paths.
- `src-tauri/src/cli_locate.rs`: CLI discovery. GUI apps on macOS start with a minimal `PATH`, so `cli_search_path()` merges the process `PATH`, the login shell's `PATH` (probed once, off the UI thread), and well-known install folders; `resolve_cli_binary` searches it and `AgentCli` hands the same list to spawned CLIs as their `PATH` (npm shims are `#!/usr/bin/env node`). Route every CLI lookup and spawn through these.
- `src-tauri/src/process.rs`: child-process draining with a hard deadline, shared by `cli.rs` and the login-shell probe.
- `src-tauri/src/cli_stub.rs`: test-only builder that writes a fake CLI as `.cmd` on Windows or an executable `sh` script elsewhere; use it instead of hand-written batch stubs.
- `scripts/build-bundle.ps1`: builds, validates, and stages one installer format (`nsis`, `msi`, `dmg`); CI runs it on both Windows and macOS runners.
- `src-tauri/src/config_io.rs` and `backup.rs`: guarded configuration writes and rollback support.
- `src-tauri/src/usage/`: read-only transcript aggregation plus caches under `.on-n-off`.
- `src-tauri/src/limits/`: the Limits screen's live subscription rate limits. Reads the CLIs' stored logins (macOS Keychain via `/usr/bin/security`, else `~/.claude/.credentials.json`; `~/.codex/auth.json`) and calls the vendors' usage endpoints over HTTPS. Never writes to or refreshes those logins (the Claude access token is memoised in-process only); the only writes are per-account snapshots of the numbers under `~/.on-n-off/limits/`, so accounts the user has signed out of stay visible. Provider problems come back as a `status` on the DTO, not an error.
- `HANDOFF.md`: Windows and macOS runtime and smoke-test expectations.

Keep provider-specific behavior behind `AgentAdapter`. Keep frontend calls behind `$lib/api`; do not invoke Tauri directly from feature components.

## Data safety

- Agent homes are real user data: `%USERPROFILE%\.claude` / `~/.claude`, `.codex`, `.gemini`, `.cursor`, and `.agents`.
- Tests must use temporary fixtures or set `ON_N_OFF_HOME` to a disposable directory. Never run a mutating test against the real user home.
- Preserve `ConfigIo` backup, atomic replacement, validation, and rollback behavior for provider configuration edits.
- CLI installs and uninstalls can have side effects outside on-n-off's rollback boundary. State this clearly when testing them and prefer throwaway inputs.
- Runtime QA is read-only unless the user explicitly authorizes a configuration mutation.
- Do not weaken validation or repair malformed fixtures merely to make a test pass.

## Performance rules

- Tauri command handlers must not perform filesystem, process, transcript, or network work on the UI thread. Make the command `async`, clone owned state before `await`, and run blocking adapter work with `tauri::async_runtime::spawn_blocking`.
- Do not hold `tauri::State` borrows or mutex guards across `await`.
- Startup loads the selected provider first. Other providers may load afterward in the background, using the existing per-provider in-flight de-duplication.
- Do not start Overview transcript aggregation before the initial selected provider finishes.
- Drain child stdout and stderr concurrently from process start; waiting for exit before reading pipes can deadlock.
- Keep the Vite entry chunk below its 500 kB warning threshold. Lazy-load large route/visualization dependencies and use appropriately sized UI assets.
- Do not hide a bundle regression by increasing the warning threshold.

## Implementation conventions

- Preserve Tauri command names and serialized request/response shapes unless a migration is explicitly approved.
- Prefer small typed helpers and existing adapter/config abstractions over provider-specific branches in commands or components.
- Keep Rust free of unsafe code. Default Clippy with `-D warnings` is the enforced lint gate; pedantic/nursery findings are advisory and should be applied deliberately.
- Keep the code and its tests platform-neutral: no hard-coded drive letters or `\` separators, gate OS-specific behavior with `cfg!(windows)` / `#[cfg(unix)]`, and remember tests run on both Windows and macOS CI runners.
- Add a regression test before changing behavior. A new test must fail for the intended reason before the fix is written.
- Preserve unrelated working-tree changes. Do not reset, overwrite, or broadly stage user work.

## Commands

Run from the repository root, in PowerShell on Windows or bash/zsh on macOS:

```sh
bun install
bun run test
bun run check
bun run build
bun run tauri dev

cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features

bun run tauri build                        # Windows: NSIS + MSI
bun run tauri build --bundles app,dmg      # macOS: .app + .dmg

./scripts/check-release-version.test.ps1   # PowerShell 7 on either platform
./scripts/build-bundle.test.ps1
./scripts/new-update-feed.test.ps1
```

The `scripts/*.ps1` helpers and their tests are path-neutral and run under PowerShell 7 on both CI legs; if `pwsh` is not installed locally, rely on CI for them.

On restricted Windows sandboxes, Vite/esbuild can fail with `spawn EPERM`. Record that failure, then rerun the exact command in an approved context; do not change code to accommodate the sandbox fault.

## Completion gate

- Run the full frontend and Rust matrices above after relevant changes.
- Boot the actual app for changes affecting IPC, startup, provider scanning, routing, or visuals. On macOS, also launch the built `.app` bundle with `open` (Finder-like minimal `PATH`) when CLI resolution changes.
- Smoke Overview, Plugins, Skills, MCP, Usage, Limits, Agent Config, Settings, search/filtering, and every provider switch without mutating live configuration. Limits makes outbound HTTPS calls and, on macOS, triggers a one-time Keychain "allow" prompt for `/usr/bin/security`.
- Verify the window remains interactive while startup work is still running.
- Stop all dev-server/app/debugger processes when QA finishes.
- Report exact commands, failures, and unresolved gates. Do not claim completion from stale or partial evidence.
