# AGENTS.md

## Project map

on-n-off is a Windows-first Tauri 2 desktop application.

- `ui/`: React 19, TypeScript, Vite, Tailwind, TanStack Router/Query/Charts.
- `src-tauri/src/commands.rs`: the IPC boundary used by `ui/src/lib/api.ts`.
- `src-tauri/src/{claude,codex,antigravity}.rs`: provider adapters.
- `src-tauri/src/config_io.rs` and `backup.rs`: guarded configuration writes and rollback support.
- `src-tauri/src/usage/`: read-only transcript aggregation plus caches under `.on-n-off`.
- `HANDOFF.md`: Windows runtime and smoke-test expectations.

Keep provider-specific behavior behind `AgentAdapter`. Keep frontend calls behind `$lib/api`; do not invoke Tauri directly from feature components.

## Data safety

- Agent homes are real user data: `%USERPROFILE%\.claude`, `.codex`, `.gemini`, and `.agents`.
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
- Add a regression test before changing behavior. A new test must fail for the intended reason before the fix is written.
- Preserve unrelated working-tree changes. Do not reset, overwrite, or broadly stage user work.

## Commands

Run from the repository root in PowerShell:

```powershell
bun install
bun run test
bun run check
bun run build
bun run tauri dev

cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features

bun run tauri build
```

On restricted Windows sandboxes, Vite/esbuild can fail with `spawn EPERM`. Record that failure, then rerun the exact command in an approved context; do not change code to accommodate the sandbox fault.

## Completion gate

- Run the full frontend and Rust matrices above after relevant changes.
- Boot the actual app for changes affecting IPC, startup, provider scanning, routing, or visuals.
- Smoke Overview, Plugins, Skills, MCP, Usage, Agent Config, Settings, search/filtering, and every provider switch without mutating live configuration.
- Verify the window remains interactive while startup work is still running.
- Stop all dev-server/app/debugger processes when QA finishes.
- Report exact commands, failures, and unresolved gates. Do not claim completion from stale or partial evidence.
