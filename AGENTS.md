# AGENTS.md

on-n-off is a Tauri 2 desktop application for Windows and macOS (Apple Silicon). It reads what
your coding agents have on disk and shows it in one place.

This file is orientation and operations: where things are, how to run them, how to finish. It is
deliberately short.

| Read before you… | Document |
| --- | --- |
| work on a subsystem you do not know | [`docs/architecture/`](docs/architecture/) |
| touch `cli_locate.rs`, `process.rs`, `scripts/`, or CI | [`OS.md`](OS.md) |
| change a provider adapter | [`PROVIDERS.md`](PROVIDERS.md) — update it in the same change |
| hand off or smoke-test a build | [`HANDOFF.md`](HANDOFF.md) |

## Project map

Each entry says what a thing is and where. The **why** lives in the module doc-comment at the top
of the file — start there, not here.

**Frontend** — React 19, TypeScript, Vite, Tailwind, TanStack Router/Query/Charts.

- `ui/src/lib/api.ts` — the only place the UI calls Tauri.
- `ui/src/features/` — one directory per screen: `agents`, `catalog`, `github`, `limits`, `notch`,
  `scope`, `session`, `settings`, `shell`, `updater`, `usage`.
- `ui/src/dev/mockIpc.ts` — `?mock[=scenario]` on a dev build swaps Tauri's IPC for synthetic
  data. Dead code in production builds. Fixtures in `githubFixtures.ts`, `limitsFixtures.ts`.

**Backend** — `src-tauri/src/`.

- `commands.rs` — the IPC boundary. `dto.rs` — the shapes that cross it.
- `{claude,codex,antigravity,cursor}.rs` — provider adapters behind `AgentAdapter` (`adapter.rs`).
- `limits/`, `github/`, `usage/`, `item_install/`, `side_notch/` — the feature subsystems.
- `limits_refresh.rs`, `read_revision.rs` — shared cached reads and how surfaces hear about them.
- `limits_monitor.rs`, `github_monitor.rs`, `monitor.rs` — background polling and notifications.
- `cli_locate.rs`, `cli.rs`, `process.rs` — finding and running provider CLIs.
- `config_io.rs`, `backup.rs` — guarded configuration writes and rollback.
- `paths.rs` — agent homes and app data paths; `ON_N_OFF_HOME` redirects them for tests.
- `cli_stub.rs` — test-only fake CLI builder (`.cmd` on Windows, `sh` script elsewhere).
- `tray.rs` — the status item on both platforms: a Limits popover on macOS, the app's own home
  on Windows. See [`OS.md`](OS.md) for what differs.
- `macos/SideNotch/` — the bundled SwiftUI notch helper, built by `native_build.rs` on macOS only.
- `side_notch/win_*.rs` — the Windows notch, which has no helper and paints its own window.

**Build and CI** — `scripts/`, `.github/workflows/`.

- `build-bundle.ps1` — builds, validates and stages one installer format (`nsis`, `dmg`).
- `read-rust-toolchain.ps1`, `prune-rust-toolchains.ps1` — see "Toolchain pinning" below.
- `ui-shots.mjs` — the screenshot harness; see "Judging visuals" below.

## Toolchain pinning

`rust-toolchain.toml` pins the compiler, and this is load-bearing for CI cost rather than for
correctness.

`Swatinem/rust-cache` hashes the rustc version into the environment portion of every cache key —
which is also its restore-key fallback — so an unpinned `stable` cold-starts every job at once on
each six-week Rust release. It also hashes *every* installed toolchain from `rustup toolchain
list`, so the runner image's own `stable` sitting beside the pin keeps the key moving whenever the
image moves.

Hence two steps, both of which must run **before** the rust-cache step:
`read-rust-toolchain.ps1` feeds `channel` to the workflows so the version lives in one place and
rejects anything that is not an exact version, and `prune-rust-toolchains.ps1` removes every other
toolchain. Bump the pin on its own pull request; the run after a bump pays one cold build per
shared key.

`.github/workflows/cache-prune.yml` keeps the Actions cache under GitHub's 10 GB per-repository
cap by deleting superseded rust-cache generations. Each generation costs roughly 1.9 GB across the
four shared keys, and eviction at the cap silently turns warm jobs cold.

## Judging visuals

`bun run ui:shots [scenes.json]` drives headless Chromium (Playwright, its own Vite on `:1425`)
through click/fill/press steps against a `?mock` build and writes retina PNGs to `.tmp/ui-shots/`.

Design and accessibility work on a screen is judged from those captures — and, for WebKit
fidelity, from a `screencapture -l <window id>` of the running app — never from reading the
markup. The notch helper is invisible to ordinary screenshots; use its `--render` path and
`scripts/check-native-notch.mjs` instead.

The Windows notch draws its own pixels, so
`cargo test --lib side_notch::win_paint::visual -- --ignored` dumps the rail, every popover and a
type specimen to `.tmp-visual/`; judge it from those and from a screen grab of the running
overlay. Its typography is not judged by eye at all — see
[`docs/architecture/side-notch.md`](docs/architecture/side-notch.md).

## Constraints

Ordinary good practice is assumed. These are the rules specific to this repo, or the ones that
have already cost someone a day.

**Data safety.** Agent homes — `~/.claude`, `.codex`, `.gemini`, `.cursor`, `.agents` — are real
user data.

- Tests use temporary fixtures or point `ON_N_OFF_HOME` at a disposable directory. Never run a
  mutating test against the real home.
- Every provider-config write goes through `ConfigIo`: backup → atomic replace → validate →
  rollback. Preserve all four.
- Never weaken validation, or repair a malformed fixture, to make a test pass.
- `limits/` never reads or redeems Claude's refresh token and never writes Claude auth; `github/`
  never writes to GitHub; `usage/` and `side_notch/` are read-only. These are promises to the
  user, not implementation details — do not add a write path silently.
- Runtime QA is read-only unless the user authorizes a mutation. CLI installs and uninstalls have
  effects outside on-n-off's rollback boundary: say so, and use throwaway inputs.

**Route through these rather than reinventing them.**

- Provider differences: `AgentAdapter`. Update `PROVIDERS.md` in the same change.
- UI → Rust: `$lib/api`. Feature components never invoke Tauri directly.
- CLI lookup and spawning: `cli_locate.rs` and `AgentCli` — a GUI app does not inherit a
  terminal's `PATH`.
- Child processes: `process.rs`. Drain stdout and stderr concurrently from the start, or a full
  pipe deadlocks.
- Fake CLIs in tests: `cli_stub.rs`.
- A read shared by more than one surface: `read_revision.rs`. Announce a replacement, never a
  read, and answer an announcement unforced — either one broken makes it a loop
  ([why](docs/architecture/shared-reads.md)).
- `item_install/` never shells out to a provider CLI.

**Performance.**

- No filesystem, process, transcript or network work on the UI thread: make the command `async`,
  clone owned state before `await`, and `spawn_blocking` the blocking adapter work. Never hold a
  `tauri::State` borrow or a mutex guard across an `await`, and never emit an event or make a
  seconds-long call while holding a lock.
- Startup loads the selected provider first, through the existing per-provider in-flight
  de-duplication rather than a second one; Overview aggregation waits for it.
- The Vite entry chunk stays under its 500 kB warning: lazy-load heavy route and visualization
  dependencies, and size UI assets to fit. Never raise the threshold to hide a regression.

**Both CI legs run everything.**

- Clippy with `-D warnings` is the enforced gate. Pedantic and nursery findings are advisory:
  apply one deliberately, never chase the list.
- No hard-coded drive letters or `\` separators, in code or tests.
- Gate an item to exactly the platforms that use it. A `cfg(any(target_os = "macos", test))` on an
  item no test calls compiles unused on the Windows *test* target and fails `-D warnings` on a leg
  you cannot reproduce locally.
- Preserve Tauri command names and serialized shapes unless a migration is approved. Snapshots
  written by older versions must still load: give new fields defaults.
- On restricted Windows sandboxes Vite/esbuild can fail with `spawn EPERM`. Record it and rerun
  the same command elsewhere; do not change code for a sandbox fault.

**Tests.**

- Write the regression test first. It must fail for the intended reason before the fix exists.
- Rust unit tests live beside their module, not inside it: `foo.rs` ends with
  `#[cfg(test)] mod tests;` and the tests live in `foo/tests.rs` (or `foo/tests/` with a
  `mod.rs`). `updater_build.rs` is also included by `build.rs` via `#[path = …]`, which moves the
  directory a plain `mod tests;` resolves against, so it pins
  `#[path = "updater_build/tests.rs"]` — do not "simplify" that away.
- Shared fixtures live next to the domain that owns them: `paths::scratch_dir`,
  `http::{serve_once, serve_once_capturing, refused_url}`, `plugin_meta::with_fetch_text`,
  `usage::pricing::with_test_fetch`, `usage::scan_cache` counters, `github/fixtures.rs`,
  `limits/claude_desktop::history_path_for_home`. Single-consumer helpers stay in that module's
  own tests file; adapter test constructors stay in the adapter files, because `item_install`
  tests use them across domains.
- Keep every test file under 1000 lines. Frontend tests stay co-located as `*.test.ts(x)`.

## Commands

Run from the repository root, in PowerShell on Windows or bash/zsh on macOS.

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
```

macOS native notch checks, after a Rust build:

```sh
xcrun swift run --package-path src-tauri/macos/SideNotch NotchCoreChecks
bun scripts/check-native-notch.mjs
```

PowerShell 7 on either platform; if `pwsh` is not installed locally, rely on CI:

```sh
./scripts/check-release-version.test.ps1
./scripts/build-bundle.test.ps1
./scripts/new-update-feed.test.ps1
./scripts/read-rust-toolchain.test.ps1
./scripts/prune-rust-toolchains.test.ps1
```

## Parallel worktree sessions

Parallel work happens in worktrees under `.worktrees/`, cut from the latest `origin/main` unless
the task depends on another base. Branches are `<change-type>/<task-slug>` — `feat`, `fix`,
`perf`, `audit`, `refactor`, `docs`, `test`, `chore`, `release` — and the folder mirrors the
branch as `.worktrees/<change-type>-<task-slug>`. The primary checkout, the one where
`.worktrees/` lives, is for creating, inspecting and integrating them, not for implementing
changes. Integrate from it with fast-forward-only updates to `main`.

Two things worktrees do **not** isolate, both of which have caused trouble: the repository's
object database, refs, remotes and stash are shared — so `git stash` is not cross-session storage
and another session's branch is not yours to rewrite; and ports, running processes, dependency
caches and the real agent homes are shared with every other session on the machine — so coordinate
dev-server and app runs, and confirm ownership before touching a worktree or branch you did not
create.

## Completion gate

- Run the full frontend and Rust matrices above after relevant changes.
- Boot the actual app for changes affecting IPC, startup, provider scanning, routing or visuals.
  On macOS, when CLI resolution changes, also launch the built `.app` with `open` — a Finder-like
  minimal `PATH` is the case that breaks.
- For visual or accessibility changes, run `bun run ui:shots` and look at the captures, before and
  after.
- Smoke Overview, Plugins, Skills, MCP, Usage, Limits, Pull requests, Agent Config, Settings,
  search/filtering, and every provider switch — without mutating live configuration. Note that
  Limits makes outbound HTTPS calls, starts `codex app-server`, and on macOS triggers a one-time
  Keychain prompt for `/usr/bin/security`; Pull requests runs `gh auth token` once and calls
  `api.github.com` on every refresh.
- Verify the window stays interactive while startup work is still running.
- Stop every dev-server, app and debugger process when QA finishes.
- Report exact commands, failures and unresolved gates. Never claim completion from stale or
  partial evidence.
