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
| Isolated sign-in | `CLAUDE_CONFIG_DIR` selects the temporary account configuration and scoped credential service. On macOS the real OS home is retained so the login Keychain remains available; the app does not change Keychain defaults. |
| CLI | `claude` (`claude.cmd` via npm on Windows) — used for install/uninstall/update/toggle: `claude plugin <action> -s user <id>` |
| Plugins | `plugins/installed_plugins.json` is the truth (version 2, `installPath`, `version`); cache under `plugins/cache/<marketplace>/<plugin>/<version>/`; marketplaces in `plugins/known_marketplaces.json` (+ `plugins/marketplaces/<name>/.claude-plugin/marketplace.json`) |
| Enable state | `settings.json` → `enabledPlugins { "<id>": bool }` |
| Skills | plugin `skills/`; user skills `~/.claude/skills/<name>/SKILL.md` |
| MCP | `~/.claude.json` → `mcpServers`, disabled when `disabled: true` **or** listed in `disabledMcpServers`; app patches that file |
| Usage / Limits | transcripts under `~/.claude/projects/**/*.jsonl` (falls back to `~/projects`); OAuth access token from macOS Keychain (`/usr/bin/security`) else `~/.claude/.credentials.json`. Limits first calls `/api/oauth/profile` with that token, verifies its account and organization against the resolved native identity file (`~/.claude/.config.json` when present, otherwise `~/.claude.json`), then calls `/api/oauth/usage`. A mismatch fails closed instead of attaching one account's usage to another. An access token past its recorded `expiresAt` is renewed the way Claude Code renews its own: the refresh token is redeemed against `platform.claude.com/v1/oauth/token` with Claude Code's client id and the login's own scopes, under both of Claude Code's refresh lock directories (`~/.claude/.oauth_refresh.lock` then `~/.claude.lock`, taken in that order, broken only once a minute stale), and the reply is written back to the source it came from with every field the reply does not carry left intact. A lock already held, or a store that turns out to hold a fresh token, yields to whoever won. Everything that can fail about the write for reasons unrelated to the reply -- resolving the Keychain entry's account, proving the credentials file's directory will take a temporary -- happens before the grant is sent, because after it the old refresh token is spent and there is no un-renewed state to fall back to; what is left afterwards is one `rename` or one `security -U`, and a failure there is reported as on-n-off having spent the login, with the reason. A refused refresh token is reported as needing a new sign-in rather than a renewal, and is not sent again while the store still holds it. Renewal now lives in `accounts/claude_renew.rs`. The opt-in account manager can also automatically remember verified native logins and activate saved renewable logins through a protected journal; it does not independently renew inactive profiles. The access token is memoised per app run and re-read once when Anthropic answers 401, because Claude Code rotates it before its recorded `expiresAt` and the memoised one stops being accepted while it still looks valid; only a freshly read login that is refused again is reported as a login problem. A successful live response is authoritative. Verified user/workspace history is a dated fallback when live refresh fails. Claude Desktop's organization-only history is not assigned to individual users; legacy snapshots remain historical without inferred ownership. A provider failure stays visible as paused refresh status and does not trigger limit notifications. The app never reads Desktop cookies or `Claude Safe Storage`. |
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
| Usage / Limits | transcripts under `~/.codex/sessions/**`. Limits starts `codex app-server --stdio` through the GUI-safe CLI resolver, fixes `CODEX_HOME` to the provider home, completes the documented initialize handshake, then calls `account/read` and `account/rateLimits/read`. Only an explicit UI refresh sets `refreshToken: true`; Codex owns OAuth, token refresh, network requests, and credential writes. Limits receives identity metadata from the account subsystem before and after the handshake, rejecting a changed user or workspace. That subsystem resolves native file/keyring/auto storage and returns only the user/workspace observation key; credential bytes never leave it. The separate subscription-date reader also decodes `tokens.id_token` for account-matched `chatgpt_subscription_active_until` and `chatgpt_subscription_last_checked` claims. A future date is labeled cached Paid through, never renewal or cancellation; elapsed dates are unavailable. Multi-bucket app-server results and remembered per-account snapshots use canonical window ids, durations, reset instants, and per-window observation times. Recent session `token_count.rate_limits` events can advance only a remembered account, and only when window id/kind, duration, and reset instant (within two seconds) identify exactly one quota window; ambiguous observations are ignored. There is no fallback to the private ChatGPT usage endpoint. |
| Subscription dates | **Code; undocumented provider data:** the account card's **Connect billing** / **Retry billing** action uses a macOS 13+ helper with pinned SweetCookieKit 0.5.2, following CodexBar's browser-cookie import and hidden WebKit capture strategy. It reads only ChatGPT/OpenAI cookies from separate Safari, Chrome, Edge, Brave, or Firefox profiles and may request Keychain or browser-file access. Saved accounts can schedule their first non-interactive import; native accounts with prior billing can also refresh daily. No interactive sign-in window or extension is used. Decoded cookies live only in the helper and its nonpersistent WebKit store; SweetCookieKit may snapshot a locked browser database in a private parent-owned temporary directory, which is removed on completion or timeout; the helper exits after each import, with a 60-second parent deadline. It validates the browser user and workspace membership before and after requesting billing; Rust validates the helper identity and native or saved account before saving. Only account id, `active_until`, and `will_renew` cross stdout; no cookie or token is persisted by this feature. Imported billing observations (including explicit empty responses) persist under `~/.on-n-off/subscriptions/codex-<sha256>.json`. Failed imports retain prior data as stale; empty successful billing suppresses local token fallback. Metadata older than 24 hours is stale; automatic attempts have a persisted 24-hour cooldown and manual refresh remains available. ID-token fallback remains future-only, account-matched, cached, and never implies renewal/cancellation. Windows has cached dates only and shows that browser import is unavailable. Forget removes billing metadata. No subscription mutations or CLI credential changes are made. |
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

Subscription billing reads share a per-account UI cache and recheck eligibility on mount and account-change events. While Limits is
visible, native accounts with prior browser billing and saved Codex profiles can refresh at most
once a day. Saved profiles can make their first non-interactive check without a manual import. Opening cards rechecks the persisted subscription state; focus does not trigger another read. The backend persists an
attempt timestamp before an automatic import, including failures, so restarting cannot bypass
the daily cooldown. Concurrent imports are suppressed; native or saved provider/user/workspace identity is checked before and
after the fetch. Automatic imports disable interactive Keychain access; a blocked or expired
session retains the saved date and waits until the next daily attempt. The explicit browser
button can retry immediately and may request Keychain access. Subscription change events
invalidate the shared UI cache; reads do not emit events. Forgetting the cached subscription
removes its cached billing observation; saved profiles remain eligible for a new check. No browser credentials are retained by the app.

Claude Max plan labels use the existing login metadata: `subscriptionType=max` with
`rateLimitTier=default_claude_max_5x` displays **Max ×5**, and
`default_claude_max_20x` displays **Max ×20**. Missing or unknown tiers keep **Max**;
other subscription types keep their own label. This requires no additional request.
The selected account uses the existing green status dot beside the main usage-window label
(or in the card header when usage is unavailable).

The Codex plan badge follows CodexBar's plan-code mapping: `pro` displays **Pro ×20**,
and `prolite` (including separator variants) displays **Pro ×5**. Other providers keep their
own plan names. The subscription badge to the left of the plan uses existing metadata only: future billing
with auto-renewal becomes **Auto-renew** and `will_renew=false` becomes **No renewal**.
The latter fills with subtly textured amber tones during its final seven days, then becomes red
with white text at the recorded expiry. An elapsed renewal date and token-only dates are
**Unconfirmed**. Hover or keyboard focus shows the exact device-local date, clock time and remaining
days, with last-known/current-status caveats when applicable. A past recorded expiry is not proof
of current account access. Missing dates have no badge. A local minute timer updates presentation;
these badges introduce no additional billing requests or polling.

## Saved subscription profiles

Limits and Settings expose explicit account controls for Claude and Codex through `AgentAdapter`
capability checks. Save current, add, sign in again, use, rename, remove, and sign out are separate
operations. Add uses official CLI sign-in in an isolated home. Before saving, it attempts a first
usage/plan read through the same provider readers, then rereads the resulting credential generation.
Only the exact provider/user/workspace observation is persisted after successful profile publication.
Usage failure does not discard sign-in or old history. A fresh scoped observation hides only its
matching legacy card; it never imports old unscoped quotas. Use changes the default native CLI
login after preserving its latest credential; it does not call logout. Native readback decides
which profile is active. A new sign-in for an already active identity is shown as ready to use.

Native Codex file, keyring, and auto storage are handled explicitly. Ephemeral or alternate
credential backends, selected Codex configuration profiles, custom native homes, environment auth
and detected forced-login policies are refused with guidance to use the official CLI. Claude's
isolated login uses the custom-home Keychain namespace on macOS; activation preserves shared MCP
OAuth and all unrelated configuration fields. Ordinary Claude activation allows running clients,
relying on Claude Code's native credential-change handling while retaining native refresh locks,
the protected journal and identity readback. Codex activation still requires closed clients.
Sign-out and explicit crash recovery require closed clients for both providers. Existing IDE and
desktop sessions are not promised immediate adoption.

Saved credentials and interrupted-switch recovery live in an encrypted vault under
`~/.on-n-off/accounts/`; the vault key is in macOS Keychain or Windows Credential Manager. No
plaintext fallback exists. Active native credentials are authoritative, and inactive profiles are
not automatically renewed. Sign-out can revoke all saved workspace logins for that user; removal
only removes on-n-off's saved copy. Abandoned isolated-login directories are cleaned only after
their lease is free and provider processes are gone.

New observation keys include both user and workspace/organization. Older snapshots still load as
historical observations and are not assigned to an inferred workspace. A successful scoped read
suppresses the old card when both its legacy provider ID and email match. This survives cache
reloads without transferring unscoped usage; other users and workspaces remain separate. Forget
removes the selected card and its superseded legacy cache. Unattributed Codex session
events cannot update a profile-scoped saved observation. Browser billing remains separate: the
reader verifies the native user and authenticated workspace membership when the browser's default
workspace differs, then rechecks the session after reading billing. An unreadable browser profile
is inconclusive, not proof that the user signed in incorrectly.

See [accounts architecture and validation boundaries](docs/architecture/accounts.md).

Saved profile names use the provider email. An optional free-text category belongs to each
provider/user/workspace profile and survives credential updates. “Remember accounts on this device”
is an explicit global opt-in for Claude and Codex, checked about every 30 seconds while the app runs.
Disabling retains saved profiles; removing one excludes it from automatic reenrollment until an
explicit Save current or Add account. Background discovery never activates a profile.

Billing dates appear directly on Limits cards. Connection and retry actions live in account
card controls and appear only when billing needs attention on a supported platform. Saved inactive
Codex accounts can verify their own browser session without activating or refreshing their CLI
login. The account vault exports identity metadata only; browser cookies stay in the existing
short-lived helper. First attempts and failures persist the daily cooldown without creating a
billing date. Cached legacy cards without a native or saved identity are display-only.

### Account Keychain access on macOS

Account reads use the same `/usr/bin/security` reader as Limits, scoped to the resolved service
and account. Each native verification still rereads the credential; no native login is cached
for this purpose. The account vault's encryption key is unlocked once per storage root and app
session, in memory only. Concurrent account/billing reads share the pending unlock. A denied
unlock is retained for background reads; an explicit account operation can retry it. Existing
vaults unlock before taking the shared storage lease, so an authorization prompt cannot cause
Codex account-lock timeouts. Initial vault/key creation remains serialized, and encrypted writes
and identity rechecks keep their existing leases.

A failed account-list unlock exposes a Retry action beside the error. It unlocks the existing
vault and reloads accounts; it does not start sign-in, create a vault, or change a CLI login.
