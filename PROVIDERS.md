# PROVIDERS.md — how each agent stores what on-n-off reads and writes

One section per provider: home folder, CLI, plugin layout, skills, MCP config, and what the app may
mutate. Adapters live in `src-tauri/src/{claude,codex,antigravity,cursor}.rs`; keep provider quirks
there behind `AgentAdapter`. Everything the app writes goes through `ConfigIo` (backup → atomic
replace → validate → rollback). Agent homes are real user data — see AGENTS.md "Constraints".

Legend: **verified** = observed on a real machine or in the provider's docs; **code** = what the
adapter assumes today (change the adapter and this file together).

## Common shape

- Home: `~/.<provider>` under `user_home()` — `ON_N_OFF_HOME` overrides the home for **all**
  providers (tests, QA fixtures). `%USERPROFILE%` on Windows, `$HOME` elsewhere.
- Plugin id: `<name>@<marketplace>`; `local` when there is no marketplace (`paths::plugin_id_parts`).
- Manifest lookup order for versions (`plugin_meta.rs`): `.cursor-plugin/plugin.json`,
  `.codex-plugin/plugin.json`, `.claude-plugin/plugin.json`, `plugin.json`, then a version-looking
  folder name.
- MCP DTO: `enabled` is derived per provider (see below); the UI toggle calls
  `AgentAdapter::set_mcp_enabled`.
- Project scope: `project.rs` reads `.claude/`, `.codex/`, `.cursor/` inside a project for skills
  and `.cursor/mcp.json` for project MCP.
- Local items (`item_install/`): on-n-off can copy individual skills (and, for Claude, subagents)
  out of a GitHub marketplace repository without the provider CLI. It downloads one
  `codeload.github.com` tarball, writes the item under `AgentAdapter::item_roots(scope)` (skills:
  `~/.claude/skills`, `~/.codex/skills`, `~/.gemini/antigravity-cli/skills`, `~/.cursor/skills`;
  project scope: the first dir of `project_skill_dirs`; agents: `~/.claude/agents` /
  `.claude/agents`, Claude only), and records provenance (repo, ref, commit sha, plugin version,
  per-file sha256) in `~/.on-n-off/installed-items.json`. Item folders stay byte-identical to
  upstream — no sidecar files, no frontmatter edits — so "modified locally" is a pure hash
  comparison. Replacing or removing an item first copies it to
  `~/.on-n-off/backups/<provider>/items/`. Update checks call
  `api.github.com/repos/{o}/{r}/commits/{ref}` (sha only) and re-download the tarball only when
  the sha moved. Public repositories only. `item_update_status` also returns each item's
  `source` (owner/repo/ref), `pluginName`, `upstreamPath`, and an `upstreamUrl` pointing at the
  installed commit on github.com; the Skills screen shows managed rows as `from owner/repo`
  with that link, opened through the `open_url` command (github.com HTTPS links only, via
  `tauri-plugin-opener`).
- Item dependencies (`item_install/deps.rs`): skills name the sibling skills they drive only in
  prose — neither `SKILL.md` nor `plugin.json` has a dependency field — so `inspect_marketplace`
  scans every text file of each entry for the names (frontmatter name and folder/file name) of
  the other entries in the marketplace and reports `dependsOn` per entry with a confidence:
  **high** when the name is used like a command or identifier (`` `/N` ``, `` `N` ``, `/N` in
  prose, `Skill(N)`, a quoted `"N"` shortly after the word "skill", `skill: N`, `--skill N`, or a
  path into the sibling's folder such as `skills/…/N/` or `../N/`); **medium** when it appears in
  a phrase (`N skill`, `the N`, `run N`, `use N`). Names shorter than three characters are
  ignored, self-mentions are dropped, a same-plugin sibling wins over a same-named entry in
  another plugin, and the highest confidence wins per target. The picker auto-adds high
  confidence dependencies (transitively) when an entry is checked and only hints at medium ones;
  nothing is forced. Each entry also reports `externalRefs` (`../…` and foreign `skills/…` paths
  the local copy will not contain) and `usesPluginRoot` (`CLAUDE_PLUGIN_ROOT` appears), and each
  plugin reports `extras` (`commands`, `hooks`, `mcp` present in the plugin folder or manifest);
  the picker shows these as an advisory pointing at Install plugin. High confidence edges are
  recorded per installed item as `source.dependsOn` (`plugin/kind/path`) in
  `installed-items.json`; older files without the field still load. This is a heuristic tuned
  on real marketplaces, not a contract: expect false positives on generic names and misses on
  unusual phrasing.

- GitHub pull requests (`github/`, `github_monitor.rs`): the Pull requests screen is not a
  provider and stays outside `AgentAdapter`. It borrows the GitHub CLI's login by running
  `gh auth token --hostname github.com` (found through `cli_locate`, memoised per app run,
  re-read once when GitHub answers 401, never written anywhere) and sends one GraphQL request
  per refresh to `api.github.com/graphql` (authored PRs narrowed by the configured scopes,
  review-requested, direct-review-requested for tagging, assigned; each with the head commit's
  `statusCheckRollup`; ~2 rate-limit points). Nothing is written to GitHub. The only on-n-off
  writes are `~/.on-n-off/github/prs.json` (the last good read, shown as stale when a refresh
  fails) and `~/.on-n-off/github/monitor.json` (the CI monitor's last-seen rollup per own PR).
  Polling pauses until GitHub's reset when a reply is rate limited or fewer than 50 points
  remain. The CI monitor watches the authored PRs the screen lists (the scoped first page of
  fifty), so PRs past that page or outside the scope are not watched. Public and private
  repositories the `gh` token can read; github.com only. **verified** (gh 2.97, macOS,
  2026-08): the token hand-over, the GraphQL reply shape, and the ~2 points per read (via
  `gh api graphql` with `rateLimit { cost }`); **code**: the pause thresholds.

## Claude (Claude Code)

| | |
|---|---|
| Home | `~/.claude` (+ `~/.claude.json` for MCP) |
| CLI | `claude` (`claude.cmd` via npm on Windows) — used for install/uninstall/update/toggle: `claude plugin <action> -s user <id>` |
| Plugins | `plugins/installed_plugins.json` is the truth (version 2, `installPath`, `version`); cache under `plugins/cache/<marketplace>/<plugin>/<version>/`; marketplaces in `plugins/known_marketplaces.json` (+ `plugins/marketplaces/<name>/.claude-plugin/marketplace.json`) |
| Enable state | `settings.json` → `enabledPlugins { "<id>": bool }` |
| Skills | plugin `skills/`; user skills `~/.claude/skills/<name>/SKILL.md` |
| MCP | `~/.claude.json` → `mcpServers`, disabled when `disabled: true` **or** listed in `disabledMcpServers`; app patches that file |
| Usage / Limits | transcripts under `~/.claude/projects/**/*.jsonl` (falls back to `~/projects`); OAuth access token from macOS Keychain (`/usr/bin/security`) else `~/.claude/.credentials.json`. Limits first calls `/api/oauth/profile` with that token, verifies its account and organization against `.claude.json`, then calls `/api/oauth/usage`. A mismatch fails closed instead of attaching one account's usage to another. on-n-off never loads or redeems the refresh token and never writes Claude auth. A successful live response is authoritative. Remembered data and Claude Desktop's version-2 `plan-usage-history.json` (`~/Library/Application Support/Claude/` on macOS; `%APPDATA%/Claude/` on Windows) are dated fallbacks only when live refresh fails; Desktop data is accepted only when its `org` exactly matches `.claude.json`'s `oauthAccount.organizationUuid`. `fh` / `sd` map to canonical session / weekly windows and have no reset times. A provider failure stays visible as paused refresh status and does not trigger limit notifications. The app never reads Desktop cookies or `Claude Safe Storage`. |
| Live sessions | `~/.claude/sessions/<pid>.json`, written by Claude Code while a session runs: `name`, `entrypoint` (`cli` → Terminal, `claude-desktop` → Desktop, `sdk-*` → SDK), `cwd`, `status` (`busy` → working, otherwise idle), `statusUpdatedAt` / `updatedAt`. A row is listed only while its pid is alive. Read-only; feeds the side notch popover. |
| Togglable | plugins (via CLI), user skills, MCP |

## Codex

| | |
|---|---|
| Home | `~/.codex`; shared skills also in `~/.agents/skills` |
| CLI | `codex` (npm) — install/uninstall through the CLI |
| Config | one file: `~/.codex/config.toml` — `[marketplaces.<name>]`, `[plugins."<name>@<mkt>"] enabled = bool`, `[[skills.config]] path/enabled`, `[mcp_servers.<id>] ... enabled = false` |
| Plugins | cache `plugins/cache/<marketplace>/<plugin>/<newest dir>`; marketplaces cloned under `.tmp/marketplaces/<name>` for git sources |
| Enable state | all in `config.toml`; app patches with `toml_edit` (`patch_toml_*`) |
| Skills | plugin `skills/`; user skills `~/.codex/skills` and `~/.agents/skills`; per-path enable rows in `[[skills.config]]` |
| Usage / Limits | transcripts under `~/.codex/sessions/**`. Limits starts `codex app-server --stdio` through the GUI-safe CLI resolver, fixes `CODEX_HOME` to the provider home, completes the documented initialize handshake, then calls `account/read` and `account/rateLimits/read`. Only an explicit UI refresh sets `refreshToken: true`; Codex owns OAuth, token refresh, network requests, and credential writes. on-n-off reads only `tokens.account_id` metadata from the app-server-confirmed `auth.json` after the handshake; it never reads or sends the Codex access token. Multi-bucket app-server results and remembered per-account snapshots use canonical window ids, durations, reset instants, and per-window observation times. Recent session `token_count.rate_limits` events can advance only a remembered account, and only when window id/kind, duration, and reset instant (within two seconds) identify exactly one quota window; ambiguous observations are ignored. There is no fallback to the private ChatGPT usage endpoint. |
| Live sessions | rollouts under `~/.codex/sessions/YYYY/MM/DD/*.jsonl` modified in the last hour (today and yesterday, newest 48): the `session_meta` line gives `id`, `cwd`, `originator` (`Codex Desktop` → Desktop, otherwise Terminal / VS Code); the last `task_started` without a later `task_complete` / `turn_aborted` in the final 128 KiB means working (stale after 15 minutes; a file written in the last two minutes counts as working even without boundary events). Read-only; feeds the side notch popover. |
| Togglable | plugins, skills, MCP — all file patches |

## Antigravity (Gemini CLI family)

| | |
|---|---|
| Home | `~/.gemini`; CLI state in `~/.gemini/antigravity-cli/` |
| CLI | `agy` — Windows native installer puts it in `%LOCALAPPDATA%\agy\bin` (verified) |
| Plugins | `antigravity-cli/plugins/*` (source `cli`) and `config/plugins/*` (source `config`); enablement merged from `antigravity-cli/{config,plugins,settings}.json` |
| Skills | `antigravity-cli/skills` (own frontmatter scanner) |
| MCP | `~/.gemini/config/mcp_config.json` → `mcpServers`, `disabled: true` honoured; app patches that file (`patch_antigravity_mcp_enabled`) |
| Togglable | plugins, MCP |

## Cursor

| | |
|---|---|
| Home | `~/.cursor` (`cli-config.json`, `plugins/`, `skills/`, `skills-cursor/` = built-ins, `projects/<slug>/` = per-project CLI state, `agents/`, `plans/`) |
| CLI | **`agent`** (canonical, verified `agent --version` = `2026.08.11-…`); `cursor-agent` is the legacy alias. Windows: `%LOCALAPPDATA%\cursor-agent\{agent,cursor-agent}.cmd` (+ `.ps1`, `versions\<v>\`), installer `irm 'https://cursor.com/install?win32=true' \| iex`. Unix: `~/.local/bin/agent` → `~/.local/share/cursor-agent/versions/<v>/…`, installer `curl https://cursor.com/install -fsS \| bash`. **Name clash:** other products (Grok CLI: `~/.grok/bin/agent.exe`) also install `agent`, so `cli_locate::find_cursor_cli` only accepts an `agent` whose path (or symlink target) contains a `cursor-agent` folder, or a launcher literally named `cursor-agent`. `cursor` on PATH is the **editor**, not the CLI. |
| Plugins | `plugins/local/<name>/` (manifest directly in the folder; docs recommend a symlink to the repo) and `plugins/cache/<marketplace-name>/<plugin>/<commit-sha>/` — note the extra **version level**; older commits are kept, finished downloads carry `.cache-complete`. Adapter picks complete → highest manifest version → newest manifest. Marketplace checkouts: `plugins/marketplaces/<host>/<owner>/<repo>/<sha>/.cursor-plugin/marketplace.json`. Installed/enabled state lives in the IDE's SQLite (`state.vscdb`, key `cursor.plugins.installedIds.*`, numeric marketplace ids) — not on disk in a file → plugins are listed **read-only** (`togglable: false`). |
| Skills | plugin `skills/`; user skills `~/.cursor/skills/<name>/SKILL.md`; `skills-cursor/` are Cursor's managed built-ins and are skipped |
| MCP | global `~/.cursor/mcp.json` (`mcpServers`, `command/args/env` or `url/headers`), project `.cursor/mcp.json`. **Listed read-only** (`enabled: true`, `togglable: false`; `set_mcp_enabled` refuses with `cursor::MCP_READ_ONLY`, and the MCP screen shows a notice pointing to the Cursor app / `agent mcp enable|disable`). Reason (verified 2026-08-18): Cursor never reads a `disabled` key from `mcp.json` — the CLI keeps per-project lists in `~/.cursor/projects/<slug>/mcp-approvals.json` / `mcp-disabled.json`, the IDE keeps its own state — so the earlier file-patch toggle (0.1.2–0.1.3) had no effect. |
| CLI config | `~/.cursor/cli-config.json` (permissions, approval mode); `CURSOR_CONFIG_DIR` relocates it but **not** the per-project state under `~/.cursor/projects/`; any CLI run touches `~/.cursor/statsig-cache.json` |
| Usage / Limits | not covered (Cursor tab reads inventory only) |
| Togglable | nothing (inventory only) |

## When adding a provider

1. Add the `AgentId` variant, `binary_name`, `display_name`, home in `paths.rs`.
2. Adapter behind `AgentAdapter`; resolve the CLI via `cli_locate::resolve_provider_cli`.
3. Every write through `ConfigIo`; a regression test with a temp home (`paths::scratch_dir`) before the change.
4. Document the layout here, and mark what is verified vs assumed.
