# Development guide

Read the relevant section before changing that surface. [AGENTS.md](../AGENTS.md)
holds repository-wide safety and workflow rules; [architecture](architecture/README.md)
locates the subsystems. The guides and PROVIDERS.md explain each module's implementation; the
source carries no comments (see [Code comments](#code-comments)).

## Existing boundaries

- Provider differences: `AgentAdapter`. Update `PROVIDERS.md` in the same change. Saved accounts
  are the exception: everything an account change does differently per provider — the native
  store, a login's shape, sign-in, preflight, clients — goes through the accounts seam
  (`accounts::Adapter`, implemented in `accounts/claude.rs`, `accounts/codex.rs` and
  `accounts/muse.rs` over `accounts/{claude,codex,muse}_store.rs`), never through `AgentAdapter`,
  which accounts reach only
  through `supports_accounts`.
- A list of providers, or a fact about one (key, names, binaries, which screens it has): drawn
  from the provider catalogue (`provider_catalogue.rs`), never written out again in Rust. In the UI
  it comes from `ui/src/lib/providers.ts`, which derives it from the catalogue fixture Rust asserts
  it writes. What only the UI states about a provider (its colours, the paths Agent Config, the
  scope bar and the install sheet print, how its plugins and MCP servers are switched and the
  notice where they cannot be) is a `Record<AgentId, …>` row in that module, so a provider without
  one does not compile. Only the notch helper still keeps its own list; `PROVIDERS.md` names it.
- UI → Rust: `$lib/api`. Feature components never invoke Tauri directly; only `ui/src/lib/api.ts`
  and the updater client (`ui/src/features/updater/tauriUpdaterClient.ts`) import a
  `@tauri-apps/*` package, which `scripts/ipc-commands.test.mjs` checks.
- App settings: read and change them through `useAppSettings()` (`ui/src/lib/useAppSettings.tsx`),
  with `patch`, never by saving a spread of a rendered copy, and change a list or map field with an
  updater over the latest document ([why](architecture/README.md#app-settings)). A new setting goes in `AppSettings` in
  `settings.rs` with a serde default, in `every_setting_away_from_its_default` and the fixtures
  under `src-tauri/src/settings/fixtures/`, then in the TypeScript `AppSettings` and
  `DEFAULT_APP_SETTINGS`; the contract tests on each side fail until both have it.
- CLI lookup and spawning: `cli_locate.rs` and `AgentCli` — a GUI app does not inherit a
  terminal's `PATH`.
- Child processes: `process.rs` for a command run to its exit; `json_line_session.rs` for a CLI
  kept running and spoken to in line-delimited JSON-RPC (Codex app-server), whose
  readers keep only their command, methods and normalisation. Drain stdout and stderr concurrently
  from the start, or a full pipe deadlocks.
- File locks: `FileLease` (`file_lease.rs`). It unlocks when dropped; closing a locked `File`
  leaves the lock with any child another thread spawned until that child execs.
- Fake CLIs in tests: `cli_stub.rs`.
- The side notch's supervisor: `side_notch/host.rs`, on both platforms. A platform difference goes
  in its `Transport` (`transport.rs` for the macOS helper, `win_transport.rs` for the Windows
  window), never in a second loop ([why](architecture/side-notch.md#one-host-two-transports)).
- A read shared by more than one surface: `read_revision.rs`. Announce a replacement, never a
  read, and answer an announcement unforced — either one broken makes it a loop
  ([why](architecture/shared-reads.md)). In the UI, read it through its source module in
  `ui/src/lib/sharedReads/`, never through a query key of your own, and force it only with that
  module's refresh. A new source needs the four changes
  [the guide lists](architecture/shared-reads.md#in-the-ui).
- `item_install/` never shells out to a provider CLI.
- The banked reset alert rule: `reset_alert.rs`, which does no I/O; `limits_monitor.rs` only
  performs the plan it returns, through an `Io` trait its tests fake. A number the UI restates
  from the rule (`ui/src/lib/appSettings.ts`) is checked against `reset_alert/fixtures/rule.json`
  by `ui/src/lib/resetAlertRule.test.ts`, and a Rust test keeps the fixture equal to the rule, so a
  change to one side fails until the other follows.
- A quota window: built only by `QuotaWindow::into_dto` (`limits/window.rs`) from the facts a
  reader maps, which decides its kind, label, range and reset spelling for every provider. Find a
  window by its id, never by its label, and compare reset times only as `ResetInstant`s, through
  `same_cycle_as` and `later_than` when a second or two apart is the same cycle
  ([why](architecture/README.md#limits)). The module is private to `limits/`, which re-exports only
  `ResetInstant`, `Observation` and `cycle_turned_over`; reading a label back (`scope_of_label`) and
  the range clamp stay inside it.
  What a surface does with a window beyond its kind (the notch's inner ring, a row that starts
  with the first message) is a field the reader sets on the window, never a label or provider
  match in the surface.
- A provider fact a Limits module asks (a legacy reading, a load step, credits spent, session
  rollouts, banked resets, a card feature): its `LimitsFacts` row in the provider catalogue,
  through `AgentId::limits_facts`, never a `match` on `AgentId` in shared Limits code. A fact that
  selects an implementation is a closed enum naming the ones that exist, and the code that runs it
  matches on that enum. A plan's label and capacity: `limits/plan.rs`, never the UI.
- How often Limits is read: `limits_refresh.rs`, which owns the check every consumer keeps
  (`check_interval`) and the failure backoff every failing read waits (`failure_backoff`). A read
  keeps its own failure count and passes its own interval; it never restates the formula. The UI's
  `LIMITS_CHECK_MINUTES` (`ui/src/lib/sharedReads/limits.ts`) is checked against
  `src-tauri/src/limits_refresh/fixtures/cadence.json` by `limitsCadence.test.ts`, and a Rust test
  keeps the fixture equal to `check_interval`.
- Which accounts a Limits column lists, the legacy history each folds in, and the saved profile and
  category beside each: the Limits read's column (`limits::column`, served by
  `limits_refresh::read_column`), never a join in the UI. Both Limits surfaces render it through
  `limitColumn`, which takes the entries alone; a fact a card needs about its saved profile is added
  to the column (and to `ProfileDto` through `Roster::of`, which the account list shares), read
  without a credential, never joined from the account list again
  ([why](architecture/accounts.md#the-limits-column)). Removing or archiving a card's account is
  one command naming that account (`remove_limits_account`, `set_limits_account_archived`, in
  `limits_refresh/account_actions.rs`), which reads the ids it takes along from the column it
  serves and announces the change; the UI sends no ids of its own and patches no cache
  ([why](architecture/accounts.md#remove-account)).
- A Limits card: `LimitCardFrame` (`ui/src/features/limits/LimitCardFrame.tsx`) draws every card's
  shell, header, tags, ⋯ menu and footer from the card model; a card adds only its body. Which
  actions a card offers, in what order and whether each is enabled, is the card action plan's
  alone (`cardActionPlan`, `cardActionPlan.ts`), for every provider and card state: the menu
  (`ActionMenu`, `ui/src/components/`), which always starts with Check limits now and ends with
  the destructive items, and the footer: its main button, the Switch anyway confirmation and the
  banked reset controls. Each entry carries what its handler needs (the account controls, the saved
  login, the clients still running, the reading and when its headline window resets by itself, the waiting spend), and `useCardActions`
  (`cardActions.tsx`) only binds a label and a handler to each, in one switch per surface; nothing
  else adds, removes or enables an action. A card not read yet (`PendingCardFrame`) offers only
  Check limits now (`checkNowEntry`). The plan reads two facts from the card for who is signed in:
  its signed-in state (`signedIn`), for every decision about the signed-in login, and whether it
  shows the provider's own read (`leads`), which Use banked reset requires and Archive account in
  place of Use account refuses ([why](architecture/README.md#limits)). The card model and the plan
  read each per-provider feature from `LIMITS_FEATURES` (`limitFeatures.ts`), whose values name the
  implementation they turn on (`"claude"`, `"codex"`) or are `null`: it is the catalogue fixture's
  `limitsFeatures`, which Rust writes from each provider's `LimitsFacts`, read through
  `LIMITS_IMPLEMENTATIONS`, the one list of the implementations the card model has, which also
  types each feature. A new provider has a row once Rust gives it one, `limitFeatures.test.ts`
  fails when a row names an implementation that list lacks, and reusing a feature means reusing
  that implementation. Whether a saved login must sign in again is no feature: the card reads the
  profile's login state (`loginState.ts`, from `accounts/login_state.rs`), the same for every
  provider. A new header tag kind (`cardTags.ts`)
  fails both header renderers until each surface decides. The card header is a grid of the
  provider icon, the name's column and the actions (the active dot and ⋯). Its tags come in one
  order (`cardTags.ts`): the usage clock first, then the renewal or sign-in tag, then the plan.
  When `useTagsBeside` measures that they fit, they sit on the name's line, attached at the right
  and ending before the actions; otherwise they all move to their own row, starting under the
  name in the same order. The category starts under the name and its tags and runs to the
  header's right edge. The tray popover's header puts its tags at the right, in the same order.
  A remembered or paused card's status is a tag in the clock's place on the popover only: the
  Limits screen draws it in the card's body, above its windows, so its header renders no tag for it.
  `scripts/ui-shots.mjs` checks these rules, and that tags on one line sit one gap apart in their
  order: `accountHeaderIssues` in every scene that sets `limitsColumns`, and `popoverHeaderIssues`
  in every scene that sets `popoverHeaders`; `bun run ui:layout` runs only those scenes (Visual
  verification).
  `LimitCardFrame.test.tsx` runs the card contract once for every Limits provider,
  `limitCards.test.ts` pins each provider's features, and `cardActionPlan.test.ts` is the table of
  every provider's actions in every card state.
- What a quota window or workspace share reads at an instant (its reset cycle, used now, reached,
  its percent figure and text, its reset note, its meter level): `limits/window_at.rs`, which the limits
  monitor and the Windows painter use, the painter with the clock its window passes in, never
  `Utc::now()` of its own. The card restates it in `ui/src/lib/windowAt.ts` and the macOS helper in
  `NotchCore/Models.swift` and `Meter.swift`; all three read
  `src-tauri/src/limits/window_at/fixtures/window-cases.json`, whose rows name each surface's
  wording (`card`, `notch`), so a change to one side fails until the others follow
  ([why](architecture/README.md#limits)). A meter's ink comes from its level alone:
  `usageMeterColor` (`ui/src/lib/windowAt.ts`), `meter_color` (`side_notch/model.rs`) and
  `meterInk` (`Meter.swift`). `--warn` is for *pending*, never for a meter: it is lighter than the
  accents it would replace, so a meter stepping into it reads as cooling down just as it runs out.
- Whether a card's reading is live, last observed or unavailable, and the message and tone it
  shows: its read standing, `ReadStanding::of` (`limits/standing.rs`), worked out from the card as it
  leaves the read (each Limits column entry for the UI, `NotchProvider::current` for the notch),
  never stored on it. The card, the tray popover and both notches render the standing and never
  restate the rule, `Reading::has_observations` among it; the cases they all check are
  `limits/standing/fixtures/standing-cases.json` ([why](architecture/README.md#limits)).
- What a side notch cell shows: `frame::rail` (`side_notch/model/frame.rs`), for both notches. A
  badge, notice, tone, cap or rule a drawer decides for itself is a second copy that will drift;
  change the model, the Swift models and the golden messages `host-message.json` together
  ([why](architecture/side-notch.md#what-each-cell-shows-is-decided-once)).
- The side notch rail's sizes and frames: `rail_metrics` and the frames built from it in
  `side_notch/model.rs`, mirrored by `NotchCore/Models.swift`. Never a second set of numbers in
  the Windows painter; change a measurement in both files and in the fixture both checks read,
  `rail-cases.json` ([why](architecture/side-notch.md#on-macos)).

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

## Code comments

on-n-off source carries no code comments: rationale lives in the guide or PROVIDERS.md row that
owns the subject. `scripts/no-code-comments.test.mjs` (run by `bun test scripts/` in CI) rejects a
comment line in the Rust, TypeScript, Swift and `.mjs` script sources; compiler directives (`/// <reference …>`,
`// @ts-expect-error`, Swift's tools-version line) are exempt.

## Test structure

- Write the regression test first. It must fail for the intended reason before the fix exists.
- Rust unit tests live beside their module, not inside it: `foo.rs` ends with
  `#[cfg(test)] mod tests;` and the tests live in `foo/tests.rs` (or `foo/tests/` with a
  `mod.rs`). `updater_build.rs` is also included by `build.rs` via `#[path = …]`, which moves the
  directory a plain `mod tests;` resolves against, so it pins
  `#[path = "updater_build/tests.rs"]` — do not "simplify" that away.
- Shared fixtures live next to the domain that owns them: `paths::scratch_dir`,
  `json_line_session::Script` (the scripted JSON-line peer),
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
cargo deny --manifest-path src-tauri/Cargo.toml check advisories   # needs cargo-deny
bun audit

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
`bun run ui:layout` runs only the scenes that check a layout rule (`limitsColumns`,
`popoverHeaders`; the scenes live in `scripts/ui-scenes.mjs`) and fails rather than pass with
nothing to check. CI runs it as the `layout` check on Linux and keeps the captures as the
`layout-captures` artifact when it fails. It is a required check: a PR that breaks a layout rule
cannot merge. Linux has none of the app's fonts, so CI measures with fallback fonts; when it fails
but a local run passes, judge the failure from the artifact's captures before changing a rule.

Design and accessibility work on a screen is judged from those captures — and, for WebKit
fidelity, from a `screencapture -l <window id>` of the running app — never from reading the
markup. The notch helper is invisible to computer-use screenshots; use its `--render` path and
`scripts/check-native-notch.mjs` for its layout, and a `screencapture -R` of the helper on screen
for its Liquid Glass rail and popovers, which `--render` draws in solid ink.

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
  for Codex and for Muse (its key-mint call to `api.meta.ai` with the stored device login, read
  on macOS from Muse's own Keychain item through `/usr/bin/security`, which can prompt again
  only after `auth.json` changes); on-n-off itself sends nothing to Anthropic. On macOS the account controls on Limits
  read Claude Code's Keychain item through `/usr/bin/security`, which can prompt once, though the
  Claude usage read opens no credential; Pull requests runs `gh auth token` once and calls
  `api.github.com` on every refresh.
- Verify the window stays interactive while startup work is still running.
- Stop every dev-server, app and debugger process when QA finishes.
- Report exact commands, failures and unresolved gates. Never claim completion from stale or
  partial evidence.
