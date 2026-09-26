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
- Hooks (`hooks.rs`, one module for every provider the way `mcp.rs` is): one row per handler,
  **read-only everywhere** — nothing on that screen is togglable. Scope is the user's own
  configuration plus the **enabled** plugins; project overlays, `settings.local.json` and managed
  settings are deliberately out, so a row can never claim a hook that only fires inside one
  repository. Event names stay in each provider's own vocabulary (Claude's `PreToolUse`, Codex's
  `pre_tool_use`) because the two do not agree. Ids have Codex's `[hooks.state]` shape for both
  providers — `<plugin-id>:<source>:<event_snake_case>:<group>:<index>`, with an empty plugin
  segment for user settings — so Codex's enablement is a lookup rather than a reconstruction. The
  index counts within its event, so an id survives every edit that does not move its entry; two
  event keys that snake_case alike (`Stop` and `stop`) share one key here exactly as they do in
  Codex, and both rows are still listed. A row's command is the raw text of the file, newlines and
  all — the row truncates it and the tooltip shows the whole of it — while a description is
  collapsed to the single line that labels a row. Rows are ordered in Rust and the UI presents that
  order rather than re-sorting: source, then event — both compared lower-cased, with the exact
  spellings breaking a remaining tie (`sort::cmp_plugin_then_name`) — then plugin id, since two
  marketplaces can ship a plugin of one name, then their place in the file, which needs no key of
  its own because the sort is stable and every reader emits rows in file order. A file that will not
  parse contributes no rows instead of failing the tab. Claude and Codex only, which each adapter
  answers for itself through `AgentAdapter::reads_hooks`, carried to the screen as
  `AgentInfo.readsHooks`; the other two say the provider has no hooks rather than showing an empty
  list as if none were configured.
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
| Isolated sign-in | `CLAUDE_CONFIG_DIR` selects the temporary account configuration and scoped credential service, and `CLAUDE_SECURESTORAGE_CONFIG_DIR` is removed from the sign-in's environment so an inherited one cannot send the login to the user's own store. A `claude` started for the resolved native store is handed that variable exactly as on-n-off resolved it, as Claude Code hands it to the processes it starts. On macOS the real OS home is retained so the login Keychain remains available; the app does not change Keychain defaults. |
| CLI | `claude` (`claude.cmd` via npm on Windows) — used for install/uninstall/update/toggle: `claude plugin <action> -s user <id>` |
| Plugins | `plugins/installed_plugins.json` is the truth (version 2, `installPath`, `version`); cache under `plugins/cache/<marketplace>/<plugin>/<version>/`; marketplaces in `plugins/known_marketplaces.json` (+ `plugins/marketplaces/<name>/.claude-plugin/marketplace.json`) |
| Enable state | `settings.json` → `enabledPlugins { "<id>": bool }` |
| Skills | plugin `skills/`; user skills `~/.claude/skills/<name>/SKILL.md` |
| MCP | `~/.claude.json` → `mcpServers`, disabled when `disabled: true` **or** listed in `disabledMcpServers`; app patches that file. Read-only rows (`claude_mcp.rs`) add the servers that are not the user's own. Each enabled plugin's, merged the way Claude Code 2.1.281 merges them: its root `.mcp.json` (a bare server map, or one wrapped in `mcpServers`) first, then each entry of its manifest's `mcpServers` in order (an object inline, a path inside the plugin, or a list of either), a later server of one name replacing an earlier one; an MCP bundle entry (`.mcpb`, `.dxt`) is skipped, and a path that could leave the plugin (a leading `/` or `\`, a `..` segment, a segment with `:`) is refused by its text on every platform (`plugin_files.rs`, on-n-off's own rule, shared with plugin hook files). Each is `plugin:<plugin>:<server>` with `pluginId` set, `${CLAUDE_PLUGIN_ROOT}` left unexpanded; a disabled plugin contributes none. In the all-projects view, each local-scope server from `projects[*].mcpServers`: one row per definition (name, transport and source), on when any of its projects has it on, `projects` naming them; these are not live there. What switches a server off inside a project is that project's own `projects[<path>].disabledMcpServers`: Claude Code 2.1.281 reads no other list (the top-level one is never read, and `/mcp` writes the project's), naming a plugin's server by its scoped name. So a plugin's server shows on in the all-projects view whenever its plugin is enabled, and a project's view drops the local-scope rows, shows that project's servers as project rows with its list applied, and turns off the plugin servers its list names. **docs** (2026-09, Claude Code plugins reference): the manifest's three forms, `${CLAUDE_PLUGIN_ROOT}`, the scoped name. **code** (Claude Code 2.1.281's bundled reader): the merge order and bundle entries, and where `disabledMcpServers` is read and written. **verified** (2026-09, real home): a plugin `.mcp.json` as a bare map, both file shapes among marketplace plugins, and local-scope servers under `projects`. |
| Hooks | Read-only, and **merged** across sources rather than overridden: `settings.json` → `hooks`, plus each enabled plugin's hook file — the `hooks` key of `.claude-plugin/plugin.json` (a path, a list of paths, or the events inline) when it has one, `hooks/hooks.json` otherwise. Shape `hooks.<Event>[] = { matcher?, hooks: [{ type, command, timeout?, … }] }`, `type` one of `command`/`http`/`mcp_tool`/`prompt`/`agent` and assumed `command` when absent; one row per handler, with `${CLAUDE_PLUGIN_ROOT}` left unexpanded because expanding it would show a path the file does not contain. There is no per-entry name; a hook file's top-level `description` labels its rows, else the plugin's own. Claude entries have no enable switch, so they are always `enabled: true`. **verified** (2026-09, real home): the plugin file layout, the inline-manifest form, and a top-level `description`. |
| Usage / Limits | transcripts under `~/.claude/projects/**/*.jsonl` (falls back to `~/projects`) plus the copies Claude Code sets aside as `*.jsonl.superseded-<ms>`, one record per `message.id`:`requestId` taking the copy with the most `output_tokens`, one-hour cache writes priced apart from five-minute ones (see the Usage section of `docs/architecture/README.md`); OAuth access token from the store Claude Code itself reads (`accounts/claude_store.rs`), in the dirs Claude Code resolves: the config dir is `CLAUDE_CONFIG_DIR` exactly as set (never trimmed, set even when empty) else `~/.claude`, NFC-normalized; the storage dir, which holds the credentials file and the lock directories, is the config dir unless `CLAUDE_SECURESTORAGE_CONFIG_DIR` moves it (set but empty, back to `~/.claude`); the Keychain entry is `Claude Code-credentials`, suffixed with the first eight hex digits of the SHA-256 of the storage dir when either variable chose it. A dir that is not absolute, an empty `CLAUDE_CONFIG_DIR` included, is refused, since Claude Code would resolve it against a working directory on-n-off cannot know. On macOS the login is its Keychain item, through `/usr/bin/security`, found under Claude Code's own account name (`$USER`, else the login name, else `claude-code-user` when that is not a plain name) and, failing that, under the account a service-only lookup names, which keeps an item an older Claude Code filed under another account resolving. An item that parses as JSON wins even without a token, because Claude Code signs out by emptying it; an item that is not JSON, no item, or a Keychain that cannot be read (denied, unanswered, locked) leaves it to `<storage dir>/.credentials.json`, the only store on Windows. When the Keychain cannot be read and the file holds no document, the Keychain's failure is reported rather than a signed-out user: an intended difference from Claude Code, which reads that case as signed out, because a login may be behind the Keychain and "sign in again" would be the wrong advice. Nothing is written to either store then, since which one Claude Code reads next is unknown. An emptied `claudeAiOauth` (no access token), Claude Code's sign-out, is no login for the account switch either. **code** (2026-09, Claude Code 2.1.282's bundled reader): the dirs, this precedence, the account name, and the lock directories and client id below. Limits first calls `/api/oauth/profile` with that token, verifies its account and organization against the resolved native identity file (`~/.claude/.config.json` when present, otherwise `~/.claude.json`), then calls `/api/oauth/usage?cedar_ember=1&skip_spend=1`. A mismatch fails closed instead of attaching one account's usage to another. The query is the one Claude Code sends on demand for `/limit-reset`; its regular read is plain `/api/oauth/usage`. It adds a `cedar_ember` block to the same answer, whose `grants[]` each carry `resets_left` and an `ends_at` use-by date, and `skip_spend` drops the extra-usage spend figures on-n-off never showed. The optional query never decides the read: any answer to it other than a transport failure is retried as the plain read, whose answer stands (a rejected login included) with the resets unknown, so a refused query can neither hide the windows nor read as a rejected login that clears the memo or renews a saved profile. The **Banked resets** row sums `resets_left` over every grant, as Claude Code does, and shows the soonest `ends_at` still ahead among grants holding one. Zero is reported only when the block says the account holds none: eligible with no grants, or ineligible for `no_grant`, `tier`, `seat`, `tenure`, `other_experiment` or `config_off`. Everything else is unknown: no block, `unavailable`, grants that were sent but cannot be read, or no grants for a reason about the asker (`surface`, `cli_version`, `mobile`, `unknown`, or one added later), since on-n-off is not Claude Code. A successful read that is unknown keeps the count the card already had, live and saved alike, until the soonest `ends_at` it knew of passes (the Codex rule for a lapsed count); an answer, 0 included, replaces it. A saved reset is never spent from on-n-off: the signed-in account's card points at Claude Code's `/limit-reset`, which claims the reset of whoever Claude Code is signed in as through `POST /api/organizations/{org}/reset_rate_limits`, so no other card names it. **code** (2026-09, Claude Code 2.1.280's bundled reader): the block's field names, the reasons and the query. **verified** (2026-09-22, on-n-off's own request on this machine): the query answers 200 with the windows and `cedar_ember: {eligible: false, ineligible_reason: "surface", grants: []}`, so Anthropic withholds saved-reset status from clients other than its own and the row stays hidden as unknown. The same day claude.ai's own `GET /api/organizations/{org}/usage?cedar_ember=1&skip_spend=1` answered `eligible: true` with one grant (`resets_left: 1`, `ends_at`, plus `resets_total`, `starts_at`, `usable_now`, `paused`, `clears`), the shape this reader expects, so the refusal is about the asker, not the account. on-n-off's answer carries no `event_props.surface`, so it does not say which surface it took on-n-off for. The only client-identifying header in Claude Code's request is its `User-Agent`; on-n-off does not imitate it. That account's Claude Code had no `tengu_cedar_ember` flag cached, so Claude Code's own `/limit-reset` would not offer the reset either. An access token past its recorded `expiresAt` is renewed the way Claude Code renews its own: the refresh token is redeemed against `platform.claude.com/v1/oauth/token` with the login's own `clientId` when it names one (else Claude Code's client id) and the login's own scopes, under Claude Code's refresh lock directories (`<storage dir>/.oauth_refresh.lock`, then the legacy lock beside the storage dir's real path, `<realpath of the storage dir>.lock`, taken in that order), and the reply is written back to the store Claude Code's next read uses with every field the reply does not carry left intact, under the `<storage dir>/.storage-write.lock` Claude Code takes around every change to its credentials (taken before the grant, held through the write). Every lock is touched every two seconds while held, as Claude Code touches its own, because nothing under them fits Claude Code's one-minute staleness: the Keychain probe allows 90 seconds for its prompt, the account lookup 30 and the write 20. A lock left behind is broken once stale: a minute for the refresh locks, 15 seconds for the storage-write lock. A lock already held, a lock the heartbeat finds taken away before the grant is sent, or a store that turns out to hold a fresh token, yields to whoever won. The renewal and the account switch share one writer, `claude_store::begin`: it takes the storage-write lock and reads the store under it, so a write-back is always a change to the document read under that lock. Proving the write comes after, and the renewal proves only once a grant is actually due, so a fresh or already-refused login found under the lock costs no Keychain lookup or temporary; the proof refuses a Keychain it could not read or a credentials file that is a link (replacing the link would leave the login where Claude Code no longer looks), and names its temporary `.credentials.json.on-n-off.<random>` so one left by a kill can be told apart. Everything that can fail about the write for reasons unrelated to the reply -- resolving the Keychain entry's account, creating a private temporary beside the credentials file -- happens before the grant is sent, because after it the old refresh token is spent and there is no un-renewed state to fall back to; what is left afterwards is one `security -U`, or the temporary synced, renamed over the file and its directory synced, and a failure there is reported as on-n-off having spent the login, with the reason. A refused refresh token is reported as needing a new sign-in rather than a renewal, and is not sent again while the store still holds it. The grant lives in `accounts/claude_renew.rs`; the store precedence, the Keychain lookups and the lock protocol in `accounts/claude_store.rs`, which the account switch uses too. The opt-in account manager can also automatically remember verified native logins and activate saved renewable logins through a protected journal; it does not independently renew inactive profiles. The access token is memoised per app run and re-read once when Anthropic answers 401, because Claude Code rotates it before its recorded `expiresAt` and the memoised one stops being accepted while it still looks valid; only a freshly read login that is refused again is reported as a login problem. A successful live response is authoritative. Verified user/workspace history is a dated fallback when live refresh fails. on-n-off does not read Claude Desktop's organization-only usage history, which cannot be attributed to a user; legacy snapshots remain historical without inferred ownership. A provider failure stays visible as paused refresh status and does not trigger limit notifications. The app never reads Desktop cookies or `Claude Safe Storage`. |
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
| Hooks | Read-only. Four sources: `~/.codex/hooks.json`, the `[hooks]` table of `config.toml`, each enabled plugin's `.codex-plugin/plugin.json` `hooks` (a path or the events inline, which may be wrapped in a second `hooks` key), and the legacy top-level `notify` argv, listed as a `Notification` row. There is **no** default plugin file: a plugin that serves both providers keeps Claude's entries in `hooks/hooks.json` and names its Codex file separately, so defaulting would list Claude's events under Codex. An `mcp_tool` handler shows `<server> · <tool>` in place of a command line. Enablement and trust live in `[hooks.state."<plugin-id>:<source>:<event_snake_case>:<group>:<index>"] { enabled, trusted_hash }`, looked up by the row's own id; a state entry that only records a hash is trusted, not disabled, and a key that matches nothing leaves the row enabled — which is what Codex does with an entry it has no state for. **verified** (2026-09, real home): the state key for plugin sources, for a plugin naming a file (`<id>:hooks/codex.json:session_start:0:0`) and for one holding its events inline (`<id>:plugin.json#hooks[0]:stop:0:0`), both manifest forms, and `notify`; **code**: the key Codex would write for `hooks.json` and for the `[hooks]` table, which has not been observed. |
| Usage / Limits | transcripts under `~/.codex/sessions/**` and `~/.codex/archived_sessions/**`, where Codex moves an archived session's rollout. Limits starts `codex app-server --stdio` through the GUI-safe CLI resolver, fixes `CODEX_HOME` to the provider home, completes the documented initialize handshake, then calls `account/read` and `account/rateLimits/read`, whose `rateLimitResetCredits` gives the banked reset count and the soonest expiry. A successful read whose `rateLimitResetCredits` is null keeps the count the card already had, the same rule Claude's saved resets follow. A saved-account refresh reads the count from the same backend body app-server does, `GET /backend-api/wham/usage`'s top-level `rate_limit_reset_credits.available_count`, and only when it is positive also asks `GET /backend-api/wham/rate-limit-reset-credits` for each reset's `status` and `expires_at`; that detail read never decides the refresh, and without it the count stands without an expiry, as in app-server. A count that is not a whole number of at least zero is unknown. The remembered snapshot applies the same rule on disk, so a read that cannot tell the count never erases the stored one. A remembered count stops at its soonest known expiry: once `nextExpiresAt` has passed, at least one reset has lapsed and what is left is unknown, so loading the snapshot drops the count, a read that cannot tell the count does not write it back, and a card already on screen hides the row and the spend button, until a read answers again. A count with no known expiry is kept. **code** (2026-09, openai/codex at `rust-v0.155.1`: `backend-client` `RateLimitStatusWithResetCredits` and `rate_limit_reset_credits_url`, app-server `account_processor` falling back to the usage count when the detail read fails); **verified** (2026-09-22, seven saved accounts on this machine, on-n-off's own User-Agent): every usage body carried the count and every detail read answered. A banked reset is spent only from an explicit click on the signed-in account's card, through `account/rateLimitResetCredit/consume`, after on-n-off confirms that the native login is the card's account both before app-server starts and once it has loaded the login. The card keeps one idempotency key until Codex gives a definite answer, so retrying a failed attempt cannot spend a second reset, and the shared Codex read is refreshed after every attempt, failed ones included. A business workspace pools its credits, so a member's own `credits.balance` reads 0; the member's share of the pool is Codex's spend control, which app-server reports on the main bucket as `individualLimit` (`limit`, `used`, `remainingPercent`, `resetsAt`) with `spendControlReached`, and the usage body as `spend_control` (`reached`, `individual_limit` with `limit`, `used`, `remaining_percent`, `reset_at`). on-n-off shows it as a **Workspace credits** meter row: how much of the share is used, what is left of it, and when it resets. The side notch draws the same share on the Codex cell's inner ring, as it draws Claude's Fable window, with the weekly limit still on the outer ring. The amounts are strings that may carry decimals, read only when they are finite numbers of at least zero, as Codex's own status line requires. The meter is Codex's own too, worked out once by the reader: full once `spendControlReached`, otherwise 100 less `remainingPercent` (`remaining_percent` in the usage body) when that is a number from 0 to 100, as the TUI's status line computes it, otherwise what is used of the limit, with a share of nothing full. The own balance is left out while it reads 0 beside a share. `spendControlReached` without an `individualLimit` says a limit was reached without saying which or how much, and is not shown. A remembered share, like a remembered balance, fills a read that reports none. Once its reset has passed, the share has renewed and reads as a quota window past its reset does: nothing used, and when it reset. It is not dropped, because the own balance of 0 it stands in for would come back. **code** (2026-09, openai/codex at `rust-v0.155.1`: `backend-client` `map_individual_limit`, `SpendControlLimitDetails`, app-server v2 `SpendControlLimitSnapshot`, the TUI's `format_credit_amount`); **corrected** (2026-09-24, one business member's account): that account has no per-member cap, so `individualLimit` is null and no share is shown; the figure its workspace analytics page shows is credits *spent*, below. A workspace member's **Credits spent** row is what the Codex app's "Credit usage history" shows a member: `GET /backend-api/wham/usage/daily-workspace-user-token-usage-breakdown?start_date=&end_date=&group_by=day`, sent with the same `Authorization: Bearer` and `ChatGPT-Account-Id` headers as `wham/usage`, for the 30 UTC days up to today (`start = today − 29`). Each day's spending is the sum of its `models[].credits`, as the app's own parser sums it, and only a response whose `units` is `credits` is read; the card leads with the last 7 days and notes the last 30. The response's `data_freshness_ts`, which can trail the read by hours, is kept with the figure but not shown on the card. It is asked only for a workspace plan, the ones Codex's `PlanType::is_workspace_account` counts (team-like, business-like, education-like and enterprise), and like the banked-reset detail read it never decides the usage read: a refusal or any other failure only leaves the figure out. The workspace-wide `daily-workspace-user-credit-usage` endpoint the app's admins read answers a member 403 and is not used. A saved account asks with its saved login's access token. The signed-in account's card, which app-server reads without handing over a token and whose saved shadow is never polled, asks with the native login's access token: on the user's decision (2026-09-24) this one read-only GET is the exception to Codex alone making requests for the signed-in account. It is sent only after app-server's read and its identity check, only for a workspace plan, and only for the card's account: for a workspace plan the identity check's one read of the native store, `accounts/native.rs` `codex_metadata_and_access`, also hands over that login's access token alone (never the refresh or id token), as an `AccessToken` that can only be read back as the header value, so the spending read costs no read of its own. A read that fails backs off per account, a poll interval doubling to an hour, so a failing endpoint costs at most one request (bounded by the 10-second HTTP timeout) per backoff period. A read on a workspace plan that could not tell what was spent, failed or backing off, keeps the remembered figure; a read on any other plan drops it, so an account that moves to a personal plan loses the stale figure. A figure without a freshness time is dated when it was read. The own balance of 0 is left out beside it, and a remembered figure fills a read that reports none, like the other figures. **code** (2026-09, openai/codex at `rust-v0.156.1`: `codex-rs/protocol/src/account.rs` `is_workspace_account`, the desktop app's usage-history query and parser); **verified** (2026-09-24, against one business member's account: the 7-day sum matched the Codex app's analytics). Only an explicit UI refresh sets `refreshToken: true`; Codex owns OAuth, token refresh and credential writes, and every network request for the signed-in account except the one spending GET above. Limits receives identity metadata from the account subsystem before and after the handshake, rejecting a changed user or workspace. That subsystem resolves native file/keyring/auto storage and returns only the user/workspace observation key; no other credential bytes leave it, and the access token leaves it only through `codex_metadata_and_access`, for the spending GET. The separate subscription-date reader also decodes `tokens.id_token` for account-matched `chatgpt_subscription_active_until` and `chatgpt_subscription_last_checked` claims. A future date is labeled cached Paid through, never renewal or cancellation; elapsed dates are unavailable. Multi-bucket app-server results and remembered per-account snapshots use canonical window ids, durations, reset instants, and per-window observation times. Codex reports some buckets no surface shows: its internal `base_model_inference` and `codex_bengalfox` buckets, matched by the id the reader gives their windows (`extra:<bucket>` or `extra:<bucket>:<slot>`), and the reserve and the retired Spark preview, matched by the model name after a label's last `·` (`gpt-reserve`, `gpt-5.3-codex-spark`, trimmed, in any case). The reader (`limits/codex.rs`) drops them as it parses, for signed-in and saved reads alike, and loading a remembered Codex snapshot drops them from files written before it did; so the cards, the account list, the limits monitor and both notches only ever see the windows a surface shows. Recent session `token_count.rate_limits` events can advance only a remembered account, and only when window id/kind, duration, and reset instant (within two seconds) identify exactly one quota window; ambiguous observations are ignored. The signed-in account's usage read has no fallback to the private ChatGPT usage endpoint; the spending GET is the only ChatGPT backend request on-n-off makes for it. The same response carries `rateLimitUpsell`, a banner the backend owns entirely: the app-server forwards it as untyped JSON (its nested keys stay snake_case) or nulls it when the account does not match the active login. on-n-off reads exactly one thing out of it — the call to action whose `action` is `buy_reset` — and shows it as a **Paid reset** row with the price, when the banner names one under `price.{amount_minor_units,currency}`. A currency is read only as three ISO 4217 letters and an amount only as a whole count of minor units within a sane bound; anything else leaves the offer priceless rather than the row absent. Nothing is ever bought, linked or opened: the purchase lives on the provider's own site. The offer is never written to a snapshot, never merged from a remembered read and never counts as an observation, because it is withdrawn the moment the account is under its limit again. **verified** (2026-09, openai/codex at `rust-v0.154.0` and the installed binaries' own generated bindings): `rate_limit_upsell` is `Option<serde_json::Value>` on the app-server response, gated all-or-nothing by an account match, with an in-repo test pinning verbatim passthrough; **code**: the `buy_reset` action and the `price` object, which the ChatGPT desktop app parses from the same backend payload, have not been observed on this machine. |
| Subscription dates | **Code; undocumented provider data:** two sources. The term comes from `GET https://chatgpt.com/backend-api/subscriptions?account_id=<workspace>` with the login's own access token, the endpoint ChatGPT's billing settings read (`limits/renewal.rs`): `active_until`, `will_renew`, and a note from `cancellation_outcome`, `entitlement.cancels_at`, `entitlement.is_delinquent` or `entitlement.scheduled_plan_change`. Every Codex card asks, during its usage read, with the signed-in login's token after the app-server identity check and a saved profile's token from the vault; an answer stands for a day, a failure backs off like the spending read, and the card keeps the term it remembers in its snapshot. The fallback is the login's ID token: `chatgpt_subscription_active_until` and `chatgpt_subscription_last_checked` under its `https://api.openai.com/auth` claims, read locally (`subscription.rs`), which says how long the plan is paid for and nothing about renewal. Nothing is persisted but the term beside the card's other figures. Until v0.18.1 a macOS helper read the term from the browser's ChatGPT session instead; it needed the browser's Keychain item, which asked again on every ad-hoc build, and could only ever see the one account the browser was signed in to; the dates it kept under `~/.on-n-off/subscriptions/` are removed once at startup. |
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
| Hooks | None. The adapter returns no rows and the screen says the provider has no hooks, rather than showing an empty list as if none were configured. |
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
| Hooks | None, as for Antigravity. |
| Togglable | nothing (inventory only) |

## When adding a provider

1. Add the `AgentId` variant, `binary_name`, `display_name`, home in `paths.rs`.
2. Adapter behind `AgentAdapter`; resolve the CLI via `cli_locate::resolve_provider_cli`.
3. Every write through `ConfigIo`; a regression test with a temp home (`paths::scratch_dir`) before the change.
4. Document the layout here, and mark what is verified vs assumed.

Subscription dates share a per-account UI cache that account changes invalidate. The read is local: the
ID token of the signed-in login or of the saved profile, decoded in process, with no request of its own.

Claude Max plan labels use the existing login metadata: `subscriptionType=max` with
`rateLimitTier=default_claude_max_5x` displays **Max ×5**, and
`default_claude_max_20x` displays **Max ×20**. Missing or unknown tiers keep **Max**;
other subscription types keep their own label. This requires no additional request.
The selected account uses the existing green status dot beside the main usage-window label
(or in the card header when usage is unavailable).

A Claude card also shows the subscription status the profile read already returns,
`organization.subscription_status`, beside the plan whenever it is not `active`: `past_due` and
`unpaid` read **Payment due**, `canceled`/`cancelled` **Canceled** and `expired` **Expired** (all outlined in red, like the header's other tags),
`trialing` **Trial**, and any other value is humanized; the tooltip shows the raw value. It never
decides a read and is remembered with the account like the plan. The tooltip says when the card was
checked, and adds "last known" only when the card's read did not answer, which is when the status
shown is the remembered one; a card only remembered from a snapshot reads as current, with its older
check time. No renewal or expiry date is available to the Claude Code OAuth token: the profile
carries only `subscription_status` and `subscription_created_at`, and the billing page's own
`subscription_details` read answers that token with 404 on `/api/oauth/organizations/{org}/…` and 403
on `/api/organizations/{org}/…`. **verified** (2026-09-24, one Max account, read-only probe); **code**
(Claude Code 2.1.282's profile mapper reads no renewal, expiry or cancellation field).

The Codex plan badge follows CodexBar's plan-code mapping: `pro` displays **Pro ×20**,
and `prolite` (including separator variants) displays **Pro ×5**. Other providers keep their
own plan names. The subscription badge to the left of the plan shows the term: a plan that renews is **Auto-renew**, one
that will not is **No renewal**, filling with subtly textured amber through its last seven days and red
with white text once the recorded end has passed, so the plan to spend down first stands out. An elapsed
renewal is **Unconfirmed**. Hover or keyboard focus shows the exact device-local date and clock time,
the days remaining, the note (cancelled, a scheduled plan change, a payment overdue) and when the term
was checked. A card the endpoint never answered for shows **Until <date>** from the login's ID token,
with the caveat that the token does not say whether the plan renews, and drops the badge once that date
passes. A local minute timer updates the presentation; the badges make no requests of their own.

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

Limits and the tray popover order one provider's cards the same way: the active login first; then
every account with usage left, the most usable capacity first, where the plan's multiplier weighs
the percentage its fuller main window has left (a Max ×20 at 30% outranks an untouched Max ×5);
then the accounts that are out of usage, the one whose last full window resets soonest first;
accounts with no usage known last. Usage left always outranks waiting for a reset, and equal
ranks keep the backend's order, newest observation first. The rule lives in
`ui/src/features/limits/accountCards.ts` on the shared `usageLeft` / `usableAgainAt` /
`planMultiplier` helpers; the backend itself still hands accounts over newest first.

Native Codex file, keyring, and auto storage are handled explicitly. Ephemeral or alternate
credential backends, selected Codex configuration profiles, custom native homes (a Claude home
chosen by `CLAUDE_CONFIG_DIR`, or a store `CLAUDE_SECURESTORAGE_CONFIG_DIR` moved), environment auth
and detected forced-login policies are refused with guidance to use the official CLI. Claude's
isolated login uses the custom-home Keychain namespace on macOS; activation preserves shared MCP
OAuth and all unrelated configuration fields. Ordinary Claude activation allows running clients,
relying on Claude Code's native credential-change handling while retaining native refresh locks
(plus the config file's lock and, around the credential write, `.storage-write.lock`), the
protected journal and identity readback. Verification runs only after those locks are released. Codex activation still requires closed clients.
Sign-out and explicit crash recovery require closed clients for both providers. Existing IDE and
desktop sessions are not promised immediate adoption.

Saved credentials and interrupted-switch recovery live in an encrypted vault under
`~/.on-n-off/accounts/`; the vault key is in macOS Keychain or Windows Credential Manager. No
plaintext fallback exists. Active native credentials are authoritative, and inactive profiles are
not independently renewed after native activation. Saved-account usage polling reads both Claude
and Codex with access tokens; only never-activated isolated sign-ins own automatic vault renewal.
Saved Codex profiles use the account-scoped ChatGPT usage endpoint separately from the native
app-server reader; saved Claude profiles verify user and organization before requesting usage.
Native and saved reads order limit windows consistently: weekly, then session, then per-model.
Renewal uses an encrypted intent/reply journal and blocks activation after an ambiguous outcome. Sign-out can revoke all saved workspace logins for that user; removal
only removes on-n-off's saved copy. Abandoned isolated-login directories are cleaned only after
their lease is free and provider processes are gone.

New observation keys include both user and workspace/organization. Older snapshots still load as
historical observations and are not assigned to an inferred workspace. A successful scoped read
suppresses the old card when both its legacy provider ID and email match. This survives cache
reloads without transferring unscoped usage; other users and workspaces remain separate. Forget
removes the selected card and its superseded legacy cache. Unattributed Codex session
events cannot update a profile-scoped saved observation.

See [accounts architecture and validation boundaries](docs/architecture/accounts.md).

Saved profile names use the provider email. An optional free-text category belongs to each
provider/user/workspace profile and survives credential updates. “Remember accounts on this device”
is an explicit global opt-in for Claude and Codex, checked about every 30 seconds while the app runs.
Disabling retains saved profiles; removing one excludes it from automatic reenrollment until an
explicit Save current or Add account. Background discovery never activates a profile.

Subscription dates appear directly on Limits cards, for the signed-in login and for saved inactive Codex
accounts alike, read from each login's own token without activating or refreshing the CLI login. Cached
legacy cards without a native or saved identity are display-only.

### Account Keychain access on macOS

Account reads use the same `/usr/bin/security` reader as Limits, and find Claude Code's item the
same way: under Claude Code's own account name first, then under the account a service-only
lookup names. Each native verification still rereads the credential; no native login is cached
for this purpose. Account writes go through the same tool: activation publishes a login with
`security add-generic-password -U`, and removing an isolated sign-in's scoped entry uses
`security delete-generic-password`. The app never opens a provider's item with its own Keychain
identity, which is ad-hoc signed and so changes with every build: an item the app had written
itself asked the user again on the next `security` read, and again after each update. The vault
key is the one item the app opens itself. The account vault's encryption key is unlocked once per storage root and app
session, in memory only. Concurrent account and subscription reads share the pending unlock. A denied
unlock is retained for background reads; an explicit account operation can retry it. Existing
vaults unlock before taking the shared storage lease, so an authorization prompt cannot cause
Codex account-lock timeouts. Initial vault/key creation remains serialized, and encrypted writes
and identity rechecks keep their existing leases.

A failed account-list unlock exposes a Retry action beside the error. It unlocks the existing
vault and reloads accounts; it does not start sign-in, create a vault, or change a CLI login.
