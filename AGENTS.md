# AGENTS.md

## Project map

on-n-off is a Tauri 2 desktop application for Windows and macOS (Apple Silicon).

- `ui/`: React 19, TypeScript, Vite, Tailwind, TanStack Router/Query/Charts.
- `src-tauri/src/commands.rs`: the IPC boundary used by `ui/src/lib/api.ts`.
- `src-tauri/src/{claude,codex,antigravity,cursor}.rs`: provider adapters.
- `src-tauri/src/paths.rs`: agent homes and app data paths.
- `src-tauri/src/cli_locate.rs`: CLI discovery. GUI apps on macOS start with a minimal `PATH`, and on Windows an app started before an installer ran misses the new registry `PATH`, so `cli_search_path()` merges the process `PATH`, the login shell's `PATH` (probed once, off the UI thread) or the registered user/machine `PATH` on Windows, and well-known install folders; `resolve_provider_cli` searches it per provider (Cursor's CLI is `agent`, a name other products also use, so only an `agent` inside a `cursor-agent` install folder or the legacy `cursor-agent` alias counts) and `AgentCli` hands the same list to spawned CLIs as their `PATH` (npm shims are `#!/usr/bin/env node`). Route every CLI lookup and spawn through these.
- `src-tauri/src/process.rs`: child-process draining with a hard deadline, shared by `cli.rs` and the login-shell probe.
- `src-tauri/src/cli_stub.rs`: test-only builder that writes a fake CLI as `.cmd` on Windows or an executable `sh` script elsewhere; use it instead of hand-written batch stubs.
- `scripts/build-bundle.ps1`: builds, validates, and stages one installer format (`nsis`, `dmg`); CI runs it on both Windows and macOS runners.
- `rust-toolchain.toml`: pins the compiler. `Swatinem/rust-cache` hashes the rustc version into the environment portion of every cache key, which is also its restore-key fallback, so an unpinned `stable` made each six-week Rust release cold-start every job at once. `scripts/read-rust-toolchain.ps1` feeds `channel` to the workflows so the version lives in one place, and rejects anything that is not an exact version. `scripts/prune-rust-toolchains.ps1` then removes every other toolchain: rust-cache runs `rustup toolchain list` and hashes *every* installed toolchain, so leaving the runner image's own `stable` beside the pin keeps the key moving whenever the image moves. Both steps must run before the rust-cache step. Bump it on its own pull request; the run after a bump pays one cold build per shared key.
- `.github/workflows/cache-prune.yml`: keeps the Actions cache store under GitHub's 10 GB per-repository cap by deleting superseded rust-cache generations. Each generation costs roughly 1.9 GB across the four shared keys, and eviction at the cap silently turns warm jobs cold.
- `src-tauri/src/config_io.rs` and `backup.rs`: guarded configuration writes and rollback support.
- `src-tauri/src/item_install/`: selective install of skills/subagents from a GitHub marketplace (tarball fetch, manifest inspection, atomic placement, `~/.on-n-off/installed-items.json` provenance registry, upstream update checks). Never shells out to a provider CLI; write roots come from `AgentAdapter::item_roots`.
- `src-tauri/src/usage/`: read-only transcript aggregation plus caches under `.on-n-off`.
- `src-tauri/src/limits/`: the Limits screen's live subscription rate limits. Claude reads its stored access token (macOS Keychain via `/usr/bin/security`, else `~/.claude/.credentials.json`), verifies `/api/oauth/profile`, then reads `/api/oauth/usage`; it never reads/redeems the refresh token or writes Claude auth. Codex launches the official `codex app-server`, calls `account/read` plus `account/rateLimits/read`, and lets Codex own login and refresh; on-n-off reads only `account_id` metadata from the app-server's confirmed home. The only on-n-off writes are per-account number snapshots under `~/.on-n-off/limits/`, so signed-out accounts stay visible. Provider problems come back as a `status` on the DTO, not an error.
- `src-tauri/src/github/` and `github_monitor.rs`: the Pull requests screen. Not a provider and not behind `AgentAdapter`. Auth is borrowed from the GitHub CLI (`gh auth token`, memoised per app run, re-read once on 401, never persisted); one GraphQL request per refresh reads authored (scoped), review-requested, and assigned PRs with their CI rollups and merge state (`mergeable` for conflicts, `mergeStateStatus` for ready/blocked/behind, `mergeQueueEntry`, `autoMergeRequest`; scalars and single objects, so the request cost stays ~2 points; snapshots written without them load with `Unknown`/`false` defaults); problems come back as a `status` + `hint` on the DTO backed by the last snapshot under `~/.on-n-off/github/`. The monitor (`github_monitor.rs`, built on the plumbing in `monitor.rs` shared with `limits_monitor.rs`) notifies on CI transitions of the authored PRs the screen lists — the scoped first page of fifty — and is opt-in; it polls while the window is hidden. Never writes to GitHub.
- `ui/src/dev/mockIpc.ts` and `scripts/ui-shots.mjs`: the UI screenshot harness. `?mock[=scenario]` on a dev build replaces Tauri's IPC with synthetic data (scenarios in `ui/src/dev/githubFixtures.ts`; dead code in production builds), and `bun run ui:shots [scenes.json]` drives headless Chromium (Playwright, own Vite on :1425) through click/fill/press steps and writes retina PNGs to `.tmp/ui-shots/`. Design and accessibility work on a screen is judged from those captures (and, for WebKit fidelity, a `screencapture -l <window id>` of the running app), not from the markup.
- `HANDOFF.md`: Windows and macOS runtime and smoke-test expectations.
- `OS.md`: Windows vs macOS differences that affect PATH discovery, launchers, filesystem, WebView2, packaging. Read before touching `cli_locate.rs`, `process.rs`, scripts, or CI.
- `PROVIDERS.md`: per-provider layout (home, CLI name and install path, plugin cache, skills, MCP file and toggle semantics, what is verified vs assumed). Update it together with the adapter it describes.

Keep provider-specific behavior behind `AgentAdapter`. Keep frontend calls behind `$lib/api`; do not invoke Tauri directly from feature components.

## Parallel worktree sessions

Reserve the primary checkout—the checkout where `.worktrees/` lives—for worktree creation, inspection, and integration. Do not implement task changes there. Each top-level task session owns one dedicated branch and one worktree under `.worktrees/`. Treat `git worktree list` as the source of truth; never edit another session's checkout.

A worktree isolates working files and its index, but all worktrees share the repository's object database, refs, remotes, and stash. Do not use `git stash` as cross-session storage, delete or rewrite another session's branch, or run repository-wide cleanup without confirming ownership. Worktrees also do not isolate ports, running processes, dependency caches, or the real agent homes described below. Coordinate dev-server and app runs across sessions, and keep runtime QA read-only unless the user authorizes configuration changes.

New branches use `<change-type>/<task-slug>`, and their folders use `.worktrees/<change-type>-<task-slug>`. Allowed change types are `feat`, `fix`, `perf`, `audit`, `refactor`, `docs`, `test`, `chore`, and `release`. Use a lowercase hyphenated task slug. Existing worktrees with older names can remain as-is.

Create a task worktree from the primary checkout in PowerShell. The default base is the latest `origin/main`; use another base only when the task explicitly depends on it.

```powershell
git worktree list
git fetch origin main

$WorktreeType = "perf"
$WorktreeTask = "startup-review"
$WorktreeBranch = "$WorktreeType/$WorktreeTask"
$WorktreePath = Join-Path (git rev-parse --show-toplevel) ".worktrees/$WorktreeType-$WorktreeTask"
$WorktreeBase = "origin/main"

git worktree add --no-track -b $WorktreeBranch $WorktreePath $WorktreeBase
git -C $WorktreePath rev-parse --show-toplevel
git -C $WorktreePath branch --show-current
git -C $WorktreePath status --short --branch
```

If the path or branch already exists, stop and inspect `git worktree list --porcelain`, `git branch --list $WorktreeBranch`, and the existing directory. Do not delete, prune, reuse, or repair a conflicting worktree until its ownership is confirmed. Resume an existing branch in a new worktree only after the user confirms that it belongs to the same task.

After creation, use the worktree's absolute path as the working directory for every command in that session. Before the first edit and after any resume or handoff, confirm `git rev-parse --show-toplevel`, `git branch --show-current`, and `git status --short --branch`. Run `bun install` inside that worktree when frontend tooling is needed. Do not switch branches inside a task worktree, edit the primary checkout or a sibling worktree, or silently merge or rebase onto a new base.

At handoff, report the absolute worktree path, branch, HEAD commit, clean or dirty status, verification commands and results, and push or pull-request state. Integrate from a clean primary checkout, and update `main` with fast-forward-only operations.

Clean up only after integration is confirmed and the user explicitly requests it. Stop processes that use the worktree, require a clean status, and remove the worktree without `--force`:

```powershell
git -C $WorktreePath status --short --branch
git worktree remove $WorktreePath
```

Deleting the local branch is a separate action. Use `git branch -d $WorktreeBranch` only when the user also requests branch cleanup and Git confirms it is merged. Never use `git branch -D` implicitly.

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

bun run tauri build                        # Windows: NSIS
bun run tauri build --bundles app,dmg      # macOS: .app + .dmg

./scripts/check-release-version.test.ps1   # PowerShell 7 on either platform
./scripts/build-bundle.test.ps1
./scripts/new-update-feed.test.ps1
./scripts/read-rust-toolchain.test.ps1
./scripts/prune-rust-toolchains.test.ps1
```

The `scripts/*.ps1` helpers and their tests are path-neutral and run under PowerShell 7 on both CI legs; if `pwsh` is not installed locally, rely on CI for them.

On restricted Windows sandboxes, Vite/esbuild can fail with `spawn EPERM`. Record that failure, then rerun the exact command in an approved context; do not change code to accommodate the sandbox fault.

## Completion gate

- Run the full frontend and Rust matrices above after relevant changes.
- Boot the actual app for changes affecting IPC, startup, provider scanning, routing, or visuals. On macOS, also launch the built `.app` bundle with `open` (Finder-like minimal `PATH`) when CLI resolution changes. For visual or accessibility changes, run `bun run ui:shots` and look at the captures before and after.
- Smoke Overview, Plugins, Skills, MCP, Usage, Limits, Pull requests, Agent Config, Settings, search/filtering, and every provider switch without mutating live configuration. Limits makes outbound HTTPS calls, starts `codex app-server`, and, on macOS, triggers a one-time Keychain "allow" prompt for `/usr/bin/security`. Pull requests runs `gh auth token` once and calls `api.github.com` on every refresh.
- Verify the window remains interactive while startup work is still running.
- Stop all dev-server/app/debugger processes when QA finishes.
- Report exact commands, failures, and unresolved gates. Do not claim completion from stale or partial evidence.
