# Development guide

Read the relevant section before changing that surface. [AGENTS.md](../AGENTS.md)
holds repository-wide safety and workflow rules; [architecture](architecture/README.md)
locates the subsystems. Module doc-comments explain their implementation.

## Existing boundaries

- Provider differences: `AgentAdapter`. Update `PROVIDERS.md` in the same change. Saved accounts
  are the exception: everything an account change does differently per provider — the native
  store, a login's shape, sign-in, preflight, clients — goes through the accounts seam
  (`accounts::Adapter`, implemented in `accounts/claude.rs` and `accounts/codex.rs` over
  `accounts/{claude,codex}_store.rs`), never through `AgentAdapter`, which accounts reach only
  through `supports_accounts`.
- UI → Rust: `$lib/api`. Feature components never invoke Tauri directly.
- CLI lookup and spawning: `cli_locate.rs` and `AgentCli` — a GUI app does not inherit a
  terminal's `PATH`.
- Child processes: `process.rs`. Drain stdout and stderr concurrently from the start, or a full
  pipe deadlocks.
- File locks: `FileLease` (`file_lease.rs`). It unlocks when dropped; closing a locked `File`
  leaves the lock with any child another thread spawned until that child execs.
- Fake CLIs in tests: `cli_stub.rs`.
- A read shared by more than one surface: `read_revision.rs`. Announce a replacement, never a
  read, and answer an announcement unforced — either one broken makes it a loop
  ([why](architecture/shared-reads.md)).
- `item_install/` never shells out to a provider CLI.
- A meter that shows a quota filling up: `usageMeterColor` (`ui/src/lib/limitsFormat.ts`), which the
  side notch mirrors in `NotchCore/Meter.swift` and `side_notch/model.rs`. One ramp, one set of
  endpoints, every surface. `--warn` is for *pending*, never for a meter: it is lighter than the
  accents it would replace, so a meter stepping into it reads as cooling down just as it runs out.
  Change the shape and change all three, each of which has a test pinning it.

For new settings, use `Rocker`, `Segmented` and `SettingsCard` from
`ui/src/components/`; do not introduce a parallel checkbox or switch.

## Performance

- No filesystem, process, transcript or network work on the UI thread: make the command `async`,
  clone owned state before `await`, and `spawn_blocking` the blocking adapter work. Never hold a
  `tauri::State` borrow or a mutex guard across an `await`, and never emit an event or make a
  seconds-long call while holding a lock.
- Startup loads the selected provider first, through the existing per-provider in-flight
  de-duplication rather than a second one; Overview aggregation waits for it.
- The Vite entry chunk stays under its 500 kB warning: lazy-load heavy route and visualization
  dependencies, and size UI assets to fit. Never raise the threshold to hide a regression.

## Platform and compatibility rules

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

## Test structure

- Write the regression test first. It must fail for the intended reason before the fix exists.
- Rust unit tests live beside their module, not inside it: `foo.rs` ends with
  `#[cfg(test)] mod tests;` and the tests live in `foo/tests.rs` (or `foo/tests/` with a
  `mod.rs`). `updater_build.rs` is also included by `build.rs` via `#[path = …]`, which moves the
  directory a plain `mod tests;` resolves against, so it pins
  `#[path = "updater_build/tests.rs"]` — do not "simplify" that away.
- Shared fixtures live next to the domain that owns them: `paths::scratch_dir`,
  `http::{serve_once, serve_once_capturing, refused_url, never_asked, was_asked, head_header}`, `plugin_meta::with_fetch_text`,
  `usage::pricing::{with_test_fetch, lock_rates_state}`, `usage::sources` counters,
  `github/fixtures.rs`. Single-consumer helpers stay in that module's
  own tests file; adapter test constructors stay in the adapter files, because `item_install`
  tests use them across domains.
- Keep every test file under 1000 lines. Frontend tests stay co-located as `*.test.ts(x)`.

## Commands

These are the available checks and build commands, not a request to run every
command for every edit. Use focused tests while working; the completion rules
below govern relevant code changes. Full matrices run in CI; an explicit task
restriction on local suites takes precedence.

Run from the repository root, in PowerShell on Windows or bash/zsh on macOS.

```sh
bun install
bun run test
bun run check
bun test scripts/                          # release verifier, workflow and IPC contracts
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

## Visual verification

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
[`docs/architecture/side-notch.md`](architecture/side-notch.md).

## Completion

- Run the full frontend and Rust matrices above after relevant changes.
- Boot the actual app for changes affecting IPC, startup, provider scanning, routing or visuals.
  On macOS, when CLI resolution changes, also launch the built `.app` with `open` — a Finder-like
  minimal `PATH` is the case that breaks.
- For visual or accessibility changes, run `bun run ui:shots` and look at the captures, before and
  after.
- Smoke Overview, Plugins, Skills, MCP, Usage, Limits, Pull requests, Agent Config, Settings,
  search/filtering, and every provider switch — without mutating live configuration. Note that
  Limits runs `claude -p /usage --safe-mode` and `codex app-server` and makes outbound HTTPS calls
  for Codex; on-n-off itself sends nothing to Anthropic. On macOS the account controls on Limits
  read Claude Code's Keychain item through `/usr/bin/security`, which can prompt once, though the
  Claude usage read opens no credential; Pull requests runs `gh auth token` once and calls
  `api.github.com` on every refresh.
- Verify the window stays interactive while startup work is still running.
- Stop every dev-server, app and debugger process when QA finishes.
- Report exact commands, failures and unresolved gates. Never claim completion from stale or
  partial evidence.
