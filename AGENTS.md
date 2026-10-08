# AGENTS.md

on-n-off is a Tauri desktop app for Windows and macOS (Apple Silicon) that
reads coding-agent state. `CLAUDE.md` includes this file; maintain one source.
When this checkout is under `~/Documents/personal/`, first read
`~/Documents/personal/AGENTS.md` and apply its workspace rules too.

## Read before changing a surface

Read the relevant sections, then the owning module's doc-comment.

| When working on | Read |
| --- | --- |
| An unfamiliar subsystem | [Architecture](docs/architecture/README.md) |
| Domain names in code, docs or PRs | [CONTEXT.md](CONTEXT.md) |
| Backend/UI boundaries, performance or tests | [Development guide](docs/development.md) |
| CLI lookup, process handling, scripts or CI | [OS.md](OS.md), including toolchain/cache rules |
| Provider adapters | [PROVIDERS.md](PROVIDERS.md); update it in the same change |
| Accounts, credentials or saved-account usage | [Account ownership and renewal](docs/architecture/accounts.md) |
| Shared cached reads | [Shared-read protocol](docs/architecture/shared-reads.md) |
| Visuals, accessibility, meters or the notch | [Visual verification](docs/development.md#visual-verification) and [notch design](docs/architecture/side-notch.md) |
| Handoff or smoke testing | [HANDOFF.md](HANDOFF.md) and [completion requirements](docs/development.md#completion) |
| Release work | [Release workflow](.claude/skills/release-version/SKILL.md) |

## Data safety

- Agent homes are real user data. Tests inject roots with `paths::scratch_dir`,
  `*_for(home)` or `*_in`; a test build has no user home. An ignored real-home
  probe reads only `ON_N_OFF_PROBE_HOME`. Never run a mutating test on a real home.
- Every provider-config write uses `ConfigIo`: backup, atomic replacement,
  validation, rollback. Preserve all four; never weaken validation or repair
  a malformed fixture to make a test pass.
- Active native credentials remain authoritative. Automatic remembering
  verifies logins and never activates a profile. Preserve the account guide's
  encrypted vault, native-store exceptions, renewal ownership and journals;
  secrets must not enter DTOs, ordinary backups, logs or plaintext fallbacks.
- `github/` never writes to GitHub; `usage/` and `side_notch/` remain read-only.
- Runtime QA is read-only unless the user authorizes mutation. CLI installs
  and uninstalls have effects outside rollback: disclose them and use throwaway inputs.

## Implementation boundaries

- UI calls Rust through `$lib/api`; feature components never invoke Tauri directly.
- Provider differences use `AgentAdapter`; saved-account differences use
  `accounts::Adapter`, reaching `AgentAdapter` only through `supports_accounts`.
- Reuse the [existing seams](docs/development.md#existing-boundaries) for CLI
  lookup/spawning, file leases, fake CLIs, shared reads, settings and quota meters.
- Keep filesystem, process, transcript and network work off the UI thread.
  Follow the development guide's async and lock rules.
- Preserve Tauri command names and serialized shapes unless a migration is
  approved. Give new fields defaults so old snapshots still load.
- Keep both platforms building. Clippy with `-D warnings` is the enforced gate;
  pedantic/nursery findings are advisory. Do not raise Vite's 500 kB entry-chunk
  threshold to hide a regression.

## Worktrees and integration

Before starting work or inspecting/integrating a branch, fetch origin and
fast-forward local `main` in the primary checkout. Preserve any uncommitted
changes as a patch or a named stash restored with `apply` if they prevent sync.

Implement in `.worktrees/<change-type>-<task-slug>` on
`<change-type>/<task-slug>`, based on current `origin/main` unless the task
needs another base. Types are `feat`, `fix`, `perf`, `audit`, `refactor`, `docs`,
`test`, `chore`, `release`. The primary checkout is for creating, inspecting
and integrating worktrees, not implementation. Integrate to `main` fast-forward only.

Refs, objects, remotes and stash are shared: stash is not cross-session storage,
and another session's branch is not yours to rewrite. Ports, processes, caches
and real homes are shared too. Coordinate app/server runs and confirm ownership
before touching another session's worktree, branch or process.

## Verification and completion

Write regression tests first and observe the intended failure before fixing it.
Keep test files under 1000 lines; follow the [test layout and fixture rules](docs/development.md#test-structure).
Use [focused commands](docs/development.md#commands) while working, and apply the
[completion requirements](docs/development.md#completion) before handing off.

IPC, startup, scanning, routing and visual changes require the actual app;
macOS CLI-resolution changes also require a built `.app` launched with `open`.
Judge visuals from before/after captures, including the native notch paths.
Smoke testing must leave live configuration unchanged. Stop QA processes you
started, and report exact commands, failures and unresolved gates.

## Documentation lookup and maintenance

For library, framework, SDK, API, CLI or cloud-service documentation, use
Context7 before web search: resolve the library ID unless an exact `/org/project`
ID is given, select an authoritative match and requested version, query each
concept separately, and answer from the fetched docs. Retry weak matches.
This does not apply to refactoring, scripts from scratch, business-logic
debugging, code review or general programming concepts.

Keep universal rules and task-triggered pointers here. Put subsystem details,
harness recipes and toolchain explanations in their owning guides; update that
source instead of copying it here. Prefer executable guardrails for rules
that can be enforced mechanically.
