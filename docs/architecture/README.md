# Architecture

How on-n-off is put together, for orientation.

**This is not a source of truth. The code is.** These notes and diagrams exist so you know where
to look and which pieces talk to which; when they disagree with `src-tauri/` or `ui/`, the code is
right and this file is stale. Fix it in passing or leave it — never "fix" the code to match a
diagram here.

Where files are, how to run them, and the constraints the code must respect all live in
[`../../AGENTS.md`](../../AGENTS.md).

## What it is

A Tauri 2 desktop app for Windows and macOS (Apple Silicon) that reads what your coding agents —
Claude, Codex, Antigravity, Cursor — have on disk, and shows it in one place: installed plugins,
skills, MCP servers, token usage and cost, subscription rate limits, and your GitHub pull
requests. It is overwhelmingly a **reader**; the narrow set of things it writes is listed under
"Constraints" in AGENTS.md.

## The shape of the process

```mermaid
flowchart TB
    subgraph webviews["WebViews (React 19 + TanStack Query)"]
        main["Main window<br/>Overview · Plugins · Skills · MCP<br/>Usage · Limits · Pull requests · Settings"]
        popover["Menu-bar popover<br/>?surface=limits-popover"]
    end

    api["ui/src/lib/api.ts<br/><i>the only place the UI calls Tauri</i>"]
    cmds["commands.rs<br/><i>the IPC boundary</i>"]

    subgraph core["Rust core"]
        adapters["AgentAdapter<br/>claude · codex · antigravity · cursor"]
        features["limits/ · github/ · usage/<br/>item_install/ · side_notch/"]
        plumbing["cli_locate · process · config_io<br/>paths · http · read_revision"]
    end

    subgraph outside["Outside the app"]
        homes[("Agent homes<br/>~/.claude ~/.codex …")]
        own[("~/.on-n-off<br/>settings · snapshots · caches")]
        clis["Provider CLIs<br/>codex app-server · gh"]
        net["api.anthropic · chatgpt.com<br/>api.github.com · LiteLLM"]
    end

    notch["on-n-off-notch<br/><i>bundled SwiftUI helper, macOS only;<br/>Windows paints its own window</i>"]

    main --> api
    popover --> api
    api -->|invoke| cmds
    cmds -->|events| api
    cmds --> adapters
    cmds --> features
    adapters --> plumbing
    features --> plumbing
    plumbing --> homes
    plumbing --> own
    plumbing --> clis
    plumbing --> net
    features -.->|private pipes| notch
```

Two things to hold onto:

- **`commands.rs` is the only door.** The UI reaches Rust through `$lib/api`, which reaches
  `commands.rs`, and nothing else. Feature components never invoke Tauri directly.
- **The notch is not a WebView.** On macOS it is a bundled Swift helper in its own process: Rust
  owns its settings and feeds it snapshots over bounded private pipes, and it draws and reports
  back typed actions. No credentials cross that pipe and it opens no listener. On Windows there is
  no helper — the app paints a layered overlay window itself over in-process channels.

## Reads, and why they are shared

Almost every screen is a view over an expensive read: spawning `codex app-server`, unlocking the
Keychain, a GraphQL round trip, or a scan of transcript files. Several surfaces want the same read
at the same time, so each such read is cached once per process and shared.

```mermaid
flowchart LR
    subgraph consumers["Surfaces that want the same numbers"]
        screen["Limits screen"]
        pop["Menu-bar popover"]
        mon["limits_monitor<br/><i>notifications</i>"]
        rail["Notch rail"]
    end

    cache["limits_refresh<br/><i>one read per provider,<br/>per poll interval</i>"]
    provider["Keychain + HTTPS<br/>codex app-server"]

    screen --> cache
    pop --> cache
    mon --> cache
    rail --> cache
    cache -->|"only when stale<br/>or forced"| provider
```

The same pattern holds for pull requests (`github` + `github_monitor` + the notch's PR cell). The
consequence — that a surface holding its own copy has to be told when another one refreshes — is
the subject of [shared-reads.md](shared-reads.md), and it is the part most easily got wrong.

## Feature subsystems

Each is a directory under `src-tauri/src/`; the module doc-comment at the top of its `mod.rs` is
the real explanation. What follows is only enough to know which one you want.

| Subsystem | What it does | Notable |
| --- | --- | --- |
| `{claude,codex,antigravity,cursor}.rs` | Provider adapters behind `AgentAdapter` | Every provider difference belongs here. Layout per provider is in `PROVIDERS.md`. |
| `scanner.rs`, `plugin_meta.rs`, `mcp.rs` | Plugins, skills and MCP servers for a provider | Feeds Overview, Plugins, Skills, MCP. |
| `usage/` | Token and cost aggregation from transcripts | Read-only. See below. |
| `limits/` + `limits_refresh.rs` | Live subscription rate limits | Provider problems come back as a `status` on the DTO, never an `Err`. See below. |
| `github/` + `github_monitor.rs` | The Pull requests screen and its CI notifications | Not a provider, not behind `AgentAdapter`. See below. |
| `item_install/` | Installing skills/subagents from a GitHub marketplace | The only substantial writer. Atomic placement plus a provenance registry at `~/.on-n-off/installed-items.json`, so upstream changes stay visible after the user edits their copy. |
| `side_notch/` | The notch overlay, macOS and Windows 11 | See [side-notch.md](side-notch.md). |
| `monitor.rs` | Shared polling/notification plumbing | `limits_monitor` and `github_monitor` are both built on it; they poll while the window is hidden. |

### Limits

Each provider is read the way that provider intends, and neither login is ours to manage:

- **Claude** — read the stored access token (macOS Keychain via `/usr/bin/security`, else
  `~/.claude/.credentials.json`), verify it against `/api/oauth/profile`, then read
  `/api/oauth/usage`. The refresh token is never read or redeemed, and Claude auth is never
  written.
- **Codex** — launch the official `codex app-server` and call `account/read` plus
  `account/rateLimits/read`. Codex owns its own login and refresh; on-n-off reads only
  `account_id` metadata from the app-server's confirmed home.

Because each CLI stores one login at a time, successful reads are remembered per account (numbers
only, under `~/.on-n-off/limits/`) so an account the user has switched away from stays visible
with its last observation time rather than vanishing.

### Pull requests

One GraphQL request per refresh reads authored (scoped), review-requested and assigned PRs with
their CI rollups and merge state. Problems come back as a `status` + `hint` on the DTO, backed by the
last snapshot under `~/.on-n-off/github/`. Auth is borrowed from `gh auth token`, memoised per
app run, re-read once on a 401, never persisted.

The merge fields — `mergeable` for conflicts, `mergeStateStatus` for ready/blocked/behind,
`mergeQueueEntry`, `autoMergeRequest` — are scalars and single objects on purpose, so the request
costs about 2 rate-limit points. They are interpreted in exactly one place, `github/merge.rs`:
`classify` fills `mergeKind` on the DTO, the screen only maps kinds to badges, and the monitor's
`Seen` record takes its unknown-aware facts from the same module. Snapshots written before those
fields existed load with `Unknown`/`false` defaults.

The monitor notifies on CI, review-decision, conflict and ready-to-merge transitions of the
authored PRs the screen lists — the scoped first page of fifty. It is opt-in and polls while the
window is hidden.

### Usage

Transcript aggregation, entirely read-only, with caches under `~/.on-n-off/`. Prices come from
LiteLLM's public table (`usage/pricing.rs`), cached for a day. A scan that meets a
priceable-looking model the table lacks lets the next scan re-fetch after an hour, and the Usage
refresh button re-fetches at once. The summary cache key carries the table's fetch time, so a new
table can never serve costs computed from an old one.

## Cross-cutting plumbing

| Module | Why it exists |
| --- | --- |
| `cli_locate.rs` | A GUI app does not inherit a terminal's `PATH`. Builds one merged search list and hands it to spawned CLIs as their `PATH`. |
| `process.rs` | Child-process draining with a hard deadline; stdout and stderr drained concurrently, or a full pipe deadlocks. |
| `config_io.rs`, `backup.rs` | Every provider-config write: backup → atomic replace → validate → rollback. |
| `paths.rs` | Agent homes and app data paths. `ON_N_OFF_HOME` redirects them for tests. |
| `read_revision.rs` | Tells every surface when a shared cached read has been replaced. See [shared-reads.md](shared-reads.md). |
| `http.rs` | Outbound HTTPS, plus the loopback test server the limits and github suites drive. |
| `dto.rs` | The serialized shapes crossing the IPC boundary. Changing one is a compatibility event. |

## Where the state lives

```mermaid
flowchart LR
    subgraph read["Read, never written"]
        h1["~/.claude, ~/.codex,<br/>~/.gemini, ~/.cursor, ~/.agents"]
    end
    subgraph rw["Written through ConfigIo only"]
        h2["provider config files<br/>(MCP toggles, agent config)"]
    end
    subgraph ours["~/.on-n-off — ours"]
        s1["settings.json"]
        s2["side-notch.json"]
        s3["limits/ · github/ snapshots"]
        s4["usage caches"]
        s5["installed-items.json"]
    end
```

Snapshots under `~/.on-n-off/` exist so a signed-out account or an offline launch still shows the
last trustworthy numbers rather than an empty screen. They are numbers and metadata — never
credentials.
