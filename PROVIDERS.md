# PROVIDERS.md — how each agent stores what on-n-off reads and writes

One section per provider: home folder, CLI, plugin layout, skills, MCP config, and what the app may
mutate. Adapters live in `src-tauri/src/{claude,codex,antigravity,cursor}.rs`; keep provider quirks
there behind `AgentAdapter`. Everything the app writes goes through `ConfigIo` (backup → atomic
replace → validate → rollback). Agent homes are real user data — see AGENTS.md "Constraints".

Legend: **verified** = observed on a real machine or in the provider's docs; **code** = what the
adapter assumes today (change the adapter and this file together).

## Common shape

- Home: `~/.<provider>` under `user_home()` — `ON_N_OFF_HOME` overrides the home for **all**
  providers (QA fixtures). `%USERPROFILE%` on Windows, `$HOME` elsewhere. A test build has no
  user home; tests hand each adapter its root.
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
| Account store | The store Claude Code itself reads (`accounts/claude_store.rs`), which the account operations, automatic remembering and the saved accounts' homes read and write; the Claude usage read itself opens no credential (Usage / Limits, below). Its dirs are the ones Claude Code resolves: the config dir is `CLAUDE_CONFIG_DIR` exactly as set (never trimmed, set even when empty) else `~/.claude`, NFC-normalized; the storage dir, which holds the credentials file and the lock directories, is the config dir unless `CLAUDE_SECURESTORAGE_CONFIG_DIR` moves it (set but empty, back to `~/.claude`); the Keychain entry is `Claude Code-credentials`, suffixed with the first eight hex digits of the SHA-256 of the storage dir when either variable chose it. A dir that is not absolute, an empty `CLAUDE_CONFIG_DIR` included, is refused, since Claude Code would resolve it against a working directory on-n-off cannot know. On macOS the login is its Keychain item, through `/usr/bin/security`, found under Claude Code's own account name (`$USER`, else the login name, else `claude-code-user` when that is not a plain name) and, failing that, under the account a service-only lookup names, which keeps an item an older Claude Code filed under another account resolving. An item that parses as JSON wins even without a token, because Claude Code signs out by emptying it; an item that is not JSON, no item, or a Keychain that cannot be read (denied, unanswered, locked) leaves it to `<storage dir>/.credentials.json`, the only store on Windows. When the Keychain cannot be read and the file holds no document, the Keychain's failure is reported rather than a signed-out user: an intended difference from Claude Code, which reads that case as signed out, because a login may be behind the Keychain and "sign in again" would be the wrong advice. Nothing is written to either store then, since which one Claude Code reads next is unknown. An emptied `claudeAiOauth` (no access token), Claude Code's sign-out, is no login for the account switch either. The signed-in account is the `oauthAccount` record in the identity file Claude Code rewrites at every sign-in: `.config.json` in the config dir when an older Claude Code left one, else `.claude.json` in the config dir `CLAUDE_CONFIG_DIR` chose, or `~/.claude.json`. **code** (2026-09, Claude Code 2.1.282's bundled reader): the dirs, this precedence, the account name, and the lock directories below. on-n-off never renews a Claude login: Claude Code renews each one itself. A write still takes the locks Claude Code takes around renewing and changing its login, so the two take turns: an account change holds Claude Code's refresh lock directories (`<storage dir>/.oauth_refresh.lock`, then the legacy lock beside the storage dir's real path, `<realpath of the storage dir>.lock`, taken in that order) and the identity file's lock, and the credential write holds the `<storage dir>/.storage-write.lock` Claude Code takes around every change to its credentials. Every lock is touched every two seconds while held, as Claude Code touches its own, because nothing under them fits Claude Code's one-minute staleness: the Keychain probe allows 90 seconds for its prompt, the account lookup 30 and the write 20. A lock left behind is broken once stale: a minute for the refresh locks, 10 seconds for the identity file's lock, 15 for the storage-write lock. A lock already held yields to whoever holds it. Every credential write goes through one writer, `claude_store::begin`: it takes the storage-write lock and reads the store under it, so a write is always a change to the document read under that lock. The write is proven before anything is written: the proof refuses a Keychain it could not read or a credentials file that is a link (replacing the link would leave the login where Claude Code no longer looks), and names its temporary `.credentials.json.on-n-off.<random>` so one left by a kill can be told apart. What is left is one `security -U`, or the temporary synced, renamed over the file and its directory synced. |
| CLI | `claude` (`claude.cmd` via npm on Windows) — used for install/uninstall/update/toggle: `claude plugin <action> -s user <id>` |
| Plugins | `plugins/installed_plugins.json` is the truth (version 2, `installPath`, `version`); cache under `plugins/cache/<marketplace>/<plugin>/<version>/`; marketplaces in `plugins/known_marketplaces.json` (+ `plugins/marketplaces/<name>/.claude-plugin/marketplace.json`) |
| Enable state | `settings.json` → `enabledPlugins { "<id>": bool }` |
| Skills | plugin `skills/`; user skills `~/.claude/skills/<name>/SKILL.md` |
| MCP | `~/.claude.json` → `mcpServers`, disabled when `disabled: true` **or** listed in `disabledMcpServers`; app patches that file. Read-only rows (`claude_mcp.rs`) add the servers that are not the user's own. Each enabled plugin's, merged the way Claude Code 2.1.281 merges them: its root `.mcp.json` (a bare server map, or one wrapped in `mcpServers`) first, then each entry of its manifest's `mcpServers` in order (an object inline, a path inside the plugin, or a list of either), a later server of one name replacing an earlier one; an MCP bundle entry (`.mcpb`, `.dxt`) is skipped, and a path that could leave the plugin (a leading `/` or `\`, a `..` segment, a segment with `:`) is refused by its text on every platform (`plugin_files.rs`, on-n-off's own rule, shared with plugin hook files). Each is `plugin:<plugin>:<server>` with `pluginId` set, `${CLAUDE_PLUGIN_ROOT}` left unexpanded; a disabled plugin contributes none. In the all-projects view, each local-scope server from `projects[*].mcpServers`: one row per definition (name, transport and source), on when any of its projects has it on, `projects` naming them; these are not live there. What switches a server off inside a project is that project's own `projects[<path>].disabledMcpServers`: Claude Code 2.1.281 reads no other list (the top-level one is never read, and `/mcp` writes the project's), naming a plugin's server by its scoped name. So a plugin's server shows on in the all-projects view whenever its plugin is enabled, and a project's view drops the local-scope rows, shows that project's servers as project rows with its list applied, and turns off the plugin servers its list names. **docs** (2026-09, Claude Code plugins reference): the manifest's three forms, `${CLAUDE_PLUGIN_ROOT}`, the scoped name. **code** (Claude Code 2.1.281's bundled reader): the merge order and bundle entries, and where `disabledMcpServers` is read and written. **verified** (2026-09, real home): a plugin `.mcp.json` as a bare map, both file shapes among marketplace plugins, and local-scope servers under `projects`. |
| Hooks | Read-only, and **merged** across sources rather than overridden: `settings.json` → `hooks`, plus each enabled plugin's hook file — the `hooks` key of `.claude-plugin/plugin.json` (a path, a list of paths, or the events inline) when it has one, `hooks/hooks.json` otherwise. Shape `hooks.<Event>[] = { matcher?, hooks: [{ type, command, timeout?, … }] }`, `type` one of `command`/`http`/`mcp_tool`/`prompt`/`agent` and assumed `command` when absent; one row per handler, with `${CLAUDE_PLUGIN_ROOT}` left unexpanded because expanding it would show a path the file does not contain. There is no per-entry name; a hook file's top-level `description` labels its rows, else the plugin's own. Claude entries have no enable switch, so they are always `enabled: true`. **verified** (2026-09, real home): the plugin file layout, the inline-manifest form, and a top-level `description`. |
| Usage / Limits | transcripts under `~/.claude/projects/**/*.jsonl` (falls back to `~/projects`) plus the copies Claude Code sets aside as `*.jsonl.superseded-<ms>`, one record per `message.id`:`requestId` taking the copy with the most `output_tokens`, one-hour cache writes priced apart from five-minute ones (see the Usage section of `docs/architecture/README.md`). Limits is Claude Code's own usage report (`limits/claude.rs`, `limits/claude_cli.rs`): `claude -p /usage --no-session-persistence --safe-mode --output-format stream-json --verbose`, started for the user's login as the Account store row resolves it but in a config dir of on-n-off's own, `~/.on-n-off/claude-usage`: `CLAUDE_CONFIG_DIR` names that dir, and `CLAUDE_SECURESTORAGE_CONFIG_DIR` keeps the login in the user's store, empty for the default store's unscoped Keychain entry and otherwise that store's own dir, whose hash scopes the entry as before. The report also scans every transcript its config dir's `projects` kept in the last seven days to say what used the limits, with no cache and no switch but an organization's HIPAA policy (`allow_usage_transcript_scan`); in a busy user config dir that is over a gigabyte and up to half a minute per read, in on-n-off's own it is nothing. Every Claude Code the read runs knows the variable: `--safe-mode` arrived in 2.1.169, whose binary already reads `CLAUDE_SECURESTORAGE_CONFIG_DIR`, and an older one is refused for the flag before any login is read. Claude Code makes the config dir itself on its first read, which runs from the temporary dir until then. Claude Code's own profile refresh now lands in that dir's `.claude.json`; the card's account check and plan still come from the user's own `.claude.json`, which the user's own Claude Code sessions keep current. That `.claude.json` also holds Claude Code's usage cache, filed under the account its `oauthAccount` names, which is whichever account first read there: Claude Code never checks that name against the login it runs with. It answers `/usage` from a cache under a minute old without asking Anthropic, from one under an hour old when its request fails, and files a fresh answer under the same name. So before each read on-n-off removes the dir when its `.claude.json` does not name the user's account (user and organization), and Claude Code makes it again for the login it runs with; a dir that cannot be removed fails the read without running Claude Code. A read during which the user's account changed removes the dir too, since that run may have filed one account's answer under the other's name. **verified** (2026-10-06, Claude Code 2.1.291, Windows): right after a switch, the new account's card showed the previous account's report and kept it as its remembered reading; a 20-second-old cache planted under the dir's `oauthAccount` was reported in place of the login's own usage; and an emptied dir was made again under the login's own account in 8 s, with the next read in 1.5 s. **verified** (2026-10-01, Claude Code 2.1.286): the same account's report, `/usage` itself in under 0.4 s instead of 3–20 s, the user's `.claude.json` untouched, and a missing config dir made by Claude Code on the first read. The read runs with stdin closed, `DISABLE_AUTOUPDATER=1`, and without `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` or `CLAUDE_CODE_OAUTH_TOKEN`, any of which Claude Code would prefer to the store's login. Claude Code answers from `/api/oauth/usage` with no model turn, renewing its login itself when it has to, and reports the answer on its assistant event as `usage_report.rate_limits.limits[]`, the shape `/api/oauth/usage` answers with, which `parse_claude` reads (the older top-level `five_hour` / `seven_day` / `seven_day_<model>` objects are its fallback). The read opens no Claude credential, and on-n-off sends nothing to Anthropic: only the Claude Code it starts does. `--safe-mode` leaves out CLAUDE.md, skills, plugins, hooks, MCP servers and custom commands and keeps the login, so a poll starts none of the user's customizations; without it, a live probe showed the config dir's `SessionStart` hooks running and two of its MCP servers starting. A Claude Code that does not know the flag (older than June 2026; it answers `unknown option '--safe-mode'`) is never run without it, and the card fails with "Update Claude Code: this version cannot report usage without starting your hooks and MCP servers." **docs** (2026-09, code.claude.com CLI reference and "Debug your config"): what `--safe-mode` disables, and that it keeps authentication. **verified** (2026-09-29, Claude Code 2.1.285): the same, live. The card's account is the one the identity file's `oauthAccount` names before the read (Account store, above), which must still name the same user and organization once Claude Code has answered, or the card fails with "The signed-in Claude account changed while its usage was read." Its id is the scoped key (`profile:…`) its saved profile's card has when the organization is known, else the account's own id. The plan is `oauthAccount.organizationType` at its `organizationRateLimitTier` (plan labels, below). When Claude Code prints no report, or one with no window it can read, its `claude auth status --json`, which reads only what is stored and asks Anthropic nothing, tells a store signed out (`loggedIn: false`: the card reads signed out, "Sign in with `claude` to see subscription limits.") from a report that could not be had ("Claude Code reported no usage."); a failed exit or no answer within 90 seconds reads "Claude Code could not report usage.", a `claude` that cannot be found reads "Install Claude Code and sign in with `claude` to see subscription limits." on the signed-in card and "Claude Code is not installed." on a saved one. The deadline is generous because Claude Code killed in the middle of a renewal could lose the refresh token it was just issued. A failed read keeps the card's account, so the reading it remembers stands in. The same read serves a saved account in its home and a sign-in's first usage (Saved subscription profiles, below). **verified** (2026-09-29, Claude Code 2.1.284, on this machine): 0 turns, 0 tokens, no API time and no cost; the report, the `oauthAccount` fields and no session transcript. `usage_report` is not in Anthropic's documentation. The report carries no subscription status and no saved resets, so a Claude card shows neither (below), and the **Banked resets** row is Codex's alone. Until 2026-09-29 on-n-off read the stored access token itself, asked `/api/oauth/profile` and `/api/oauth/usage?cedar_ember=1&skip_spend=1` with it, and renewed it under Claude Code's refresh locks. Its saved-reset query was answered `cedar_ember: {eligible: false, ineligible_reason: "surface", grants: []}`, while claude.ai's own request for the same account was answered `eligible: true` with one grant (**verified** 2026-09-22), so Anthropic withholds saved-reset status from clients other than its own. The opt-in account manager can also automatically remember verified native logins and activate saved renewable logins through a protected journal; it does not independently renew inactive profiles. A successful live response is authoritative. Verified user/workspace history is a dated fallback when live refresh fails. on-n-off does not read Claude Desktop's organization-only usage history, which cannot be attributed to a user; legacy snapshots remain historical without inferred ownership. A provider failure stays visible as paused refresh status and does not trigger limit notifications. The app never reads Desktop cookies or `Claude Safe Storage`. |
| Live sessions | `~/.claude/sessions/<pid>.json`, written by Claude Code while a session runs: `name`, `entrypoint` (`cli` → Terminal, `claude-desktop` → Desktop, `sdk-*` → SDK), `cwd`, `status` (`busy` → working, otherwise idle), `statusUpdatedAt` / `updatedAt`. A row is listed only while its pid is alive. Read-only; feeds the side notch popover. |
| Togglable | plugins (via CLI), user skills, MCP |

## Codex

| | |
|---|---|
| Home | `~/.codex`; shared skills also in `~/.agents/skills` |
| Isolated sign-in | `CODEX_HOME` selects the temporary home. It has no `config.toml`, so the file store is selected and `codex login` writes the sign-in's own `auth.json` there: nothing outside the home is left to remove. |
| Account store | `cli_auth_credentials_store` in `config.toml` selects where the signed-in login lives (`accounts/codex_store.rs`): `file`, the default, is `auth.json`; `keyring` is the OS credential-store item `Codex Auth` under `cli|` and the first sixteen hex digits of the SHA-256 of the home's canonical path, read and written through `/usr/bin/security` on macOS; `auto` is that item when there is one and the file otherwise. Any other backend, and a config naming a `profile`, defer to the official client. A switch writes the login back verbatim. |
| CLI | `codex` (npm) — install/uninstall through the CLI |
| Config | one file: `~/.codex/config.toml` — `[marketplaces.<name>]`, `[plugins."<name>@<mkt>"] enabled = bool`, `[[skills.config]] path/enabled`, `[mcp_servers.<id>] ... enabled = false` |
| Plugins | cache `plugins/cache/<marketplace>/<plugin>/<newest dir>`; marketplaces cloned under `.tmp/marketplaces/<name>` for git sources |
| Enable state | all in `config.toml`; app patches with `toml_edit` (`patch_toml_*`) |
| Skills | plugin `skills/`; user skills `~/.codex/skills` and `~/.agents/skills`; per-path enable rows in `[[skills.config]]` |
| Hooks | Read-only. Four sources: `~/.codex/hooks.json`, the `[hooks]` table of `config.toml`, each enabled plugin's `.codex-plugin/plugin.json` `hooks` (a path or the events inline, which may be wrapped in a second `hooks` key), and the legacy top-level `notify` argv, listed as a `Notification` row. There is **no** default plugin file: a plugin that serves both providers keeps Claude's entries in `hooks/hooks.json` and names its Codex file separately, so defaulting would list Claude's events under Codex. An `mcp_tool` handler shows `<server> · <tool>` in place of a command line. Enablement and trust live in `[hooks.state."<plugin-id>:<source>:<event_snake_case>:<group>:<index>"] { enabled, trusted_hash }`, looked up by the row's own id; a state entry that only records a hash is trusted, not disabled, and a key that matches nothing leaves the row enabled — which is what Codex does with an entry it has no state for. **verified** (2026-09, real home): the state key for plugin sources, for a plugin naming a file (`<id>:hooks/codex.json:session_start:0:0`) and for one holding its events inline (`<id>:plugin.json#hooks[0]:stop:0:0`), both manifest forms, and `notify`; **code**: the key Codex would write for `hooks.json` and for the `[hooks]` table, which has not been observed. |
| Usage / Limits | transcripts under `~/.codex/sessions/**` and `~/.codex/archived_sessions/**`, where Codex moves an archived session's rollout. Limits starts `codex app-server --stdio` through the GUI-safe CLI resolver, fixes `CODEX_HOME` to the provider home, completes the documented initialize handshake, then calls `account/read` and `account/rateLimits/read`, whose `rateLimitResetCredits` gives the banked reset count and, for each reset, its `status`, `expiresAt` and the backend's display `title` (such as "Full reset"). The card shows the soonest expiry among the available ones; when the count is more than one and more than one of them is still ahead, the **Banked resets** row lists each instead, soonest first, with its title and when it lapses, and a reset that never expires last. The list stops at the count, as the list of resets Codex's own TUI offers stops at it (`codex-rs/tui` `reset_credit_options`, `rust-v0.159.1`), since the backend can list resets the count leaves out. **code** (2026-09, openai/codex at `rust-v0.159.1`: app-server-protocol v2 `RateLimitResetCredit.title` and backend-client `RateLimitResetCreditDetails.title`). A successful read whose `rateLimitResetCredits` is null keeps the count the card already had. A saved-account refresh reads the count from the same backend body app-server does, `GET /backend-api/wham/usage`'s top-level `rate_limit_reset_credits.available_count`, and only when it is positive also asks `GET /backend-api/wham/rate-limit-reset-credits` for each reset's `status`, `expires_at` and `title`; that detail read never decides the refresh, and without it the count stands without an expiry, as in app-server. A count that is not a whole number of at least zero is unknown. The remembered snapshot applies the same rule on disk, so a read that cannot tell the count never erases the stored one. A remembered count stops at its soonest known expiry: once `nextExpiresAt` has passed, at least one reset has lapsed and what is left is unknown, so loading the snapshot drops the count, a read that cannot tell the count does not write it back, and a card already on screen hides the row and the spend button, until a read answers again. A count with no known expiry is kept. **code** (2026-09, openai/codex at `rust-v0.155.1`: `backend-client` `RateLimitStatusWithResetCredits` and `rate_limit_reset_credits_url`, app-server `account_processor` falling back to the usage count when the detail read fails); **verified** (2026-09-22, seven saved accounts on this machine, on-n-off's own User-Agent): every usage body carried the count and every detail read answered. A banked reset is spent by hand from an explicit click on the signed-in account's card, confirmed in a dialog, through `account/rateLimitResetCredit/consume`, after on-n-off confirms that the native login is the card's account both before app-server starts and once it has loaded the login. A click is never refused for how much of the limit is left: with more than 5% of the current limit left (what is left of the fullest weekly or five-hour window), more than the lower share the account's banked reset alert names, or no telling how much, the dialog warns before it is confirmed. Codex's own app refuses instead ("Allow Codex to use resets": a reset is used only when explicitly requested, with 10% or less remaining, and never automatically; **observed** 2026-09-29 in the Codex app's settings); on-n-off warns, on the user's decision (2026-10-01). A **banked reset alert** is an opt-in per Codex account, set from its card's menu: the limits monitor acts once per weekly cycle when two polls in a row find the signed-in account at the alert's share or below (1 to 5%, default 5%), with its weekly window at least the alert's hours from renewing (default 24) and a banked reset that has not lapsed. It either notifies, or, set to use the reset automatically, notifies that the reset will be used in ten minutes; the card shows the waiting spend with a Cancel until then. At that time the monitor reads Codex afresh (a forced app-server read) and spends the reset through the same path a click takes, with a new idempotency key, only if that read still finds the signed-in account low in the same cycle; a forced read that fails leaves the spend waiting for one that answers. Unlike a click, the spend then reads `account/rateLimits/read` again in the same app-server session, after the identity check and before the consume, and refuses above the alert's share or when it cannot tell what is left. A waiting spend is held in memory, so quitting on-n-off cancels it; one found more than fifteen minutes past its time, as after the computer slept, is kept; a reset used by hand from the card clears it; and a failed spend is not retried. Codex's own app chooses never to use a reset automatically; on-n-off does so only for an account the user set to. The card keeps one idempotency key until Codex gives a definite answer, so retrying a failed attempt cannot spend a second reset, and the shared Codex read is refreshed after every attempt, failed ones included. A business workspace pools its credits, so a member's own `credits.balance` reads 0; the member's share of the pool is Codex's spend control, which app-server reports on the main bucket as `individualLimit` (`limit`, `used`, `remainingPercent`, `resetsAt`) with `spendControlReached`, and the usage body as `spend_control` (`reached`, `individual_limit` with `limit`, `used`, `remaining_percent`, `reset_at`). on-n-off shows it as a **Workspace credits** meter row: how much of the share is used, what is left of it, and when it resets. The side notch draws the same share on the Codex cell's inner ring, as it draws Claude's Fable window, with the weekly limit still on the outer ring. The amounts are strings that may carry decimals, read only when they are finite numbers of at least zero, as Codex's own status line requires. The meter is Codex's own too, worked out once by the reader: full once `spendControlReached`, otherwise 100 less `remainingPercent` (`remaining_percent` in the usage body) when that is a number from 0 to 100, as the TUI's status line computes it, otherwise what is used of the limit, with a share of nothing full. The own balance is left out while it reads 0 beside a share. `spendControlReached` without an `individualLimit` says a limit was reached without saying which or how much, and is not shown. A remembered share, like a remembered balance, fills a read that reports none. Once its reset has passed, the share has renewed and reads as a quota window past its reset does: nothing used, and when it reset. It is not dropped, because the own balance of 0 it stands in for would come back. **code** (2026-09, openai/codex at `rust-v0.155.1`: `backend-client` `map_individual_limit`, `SpendControlLimitDetails`, app-server v2 `SpendControlLimitSnapshot`, the TUI's `format_credit_amount`); **corrected** (2026-09-24, one business member's account): that account has no per-member cap, so `individualLimit` is null and no share is shown; the figure its workspace analytics page shows is credits *spent*, below. A workspace member's **Credits spent** row is what the Codex app's "Credit usage history" shows a member: `GET /backend-api/wham/usage/daily-workspace-user-token-usage-breakdown?start_date=&end_date=&group_by=day`, sent with the same `Authorization: Bearer` and `ChatGPT-Account-Id` headers as `wham/usage`, for the 30 UTC days up to today (`start = today − 29`). Each day's spending is the sum of its `models[].credits`, as the app's own parser sums it, and only a response whose `units` is `credits` is read; the card leads with the last 7 days and notes the last 30. The response's `data_freshness_ts`, which can trail the read by hours, is kept with the figure but not shown on the card. It is asked only for a workspace plan, the ones Codex's `PlanType::is_workspace_account` counts (team-like, business-like, education-like and enterprise), and like the banked-reset detail read it never decides the usage read: a refusal or any other failure only leaves the figure out. The workspace-wide `daily-workspace-user-credit-usage` endpoint the app's admins read answers a member 403 and is not used. A saved account asks with its saved login's access token. The signed-in account's card, which app-server reads without handing over a token and whose saved shadow is never polled, asks with the native login's access token: on the user's decision (2026-09-24, extended to the term on 2026-09-25) these read-only GETs are the exception to Codex alone making requests for the signed-in account. The identity check's one read of the native store, `accounts/codex_store.rs` `metadata_and_access`, also hands over that login's access token alone (never the refresh or id token), as an `AccessToken` that can only be read back as the header value, so neither read costs a read of its own. After app-server's read and that check, the gate both Codex reads ask their backend figures through (`limits/codex.rs` `backend_figures`) uses the token only for the card's account, one read-only GET per figure: the term (Subscription dates, below) and, only on a workspace plan, this spending read. A read that fails backs off per account, a poll interval doubling to an hour, so a failing endpoint costs at most one request (bounded by the 10-second HTTP timeout) per backoff period. A read on a workspace plan that could not tell what was spent, failed or backing off, keeps the remembered figure; a read on any other plan drops it, so an account that moves to a personal plan loses the stale figure. A figure without a freshness time is dated when it was read. The own balance of 0 is left out beside it, and a remembered figure fills a read that reports none, like the other figures. **code** (2026-09, openai/codex at `rust-v0.156.1`: `codex-rs/protocol/src/account.rs` `is_workspace_account`, the desktop app's usage-history query and parser); **verified** (2026-09-24, against one business member's account: the 7-day sum matched the Codex app's analytics). Only an explicit UI refresh sets `refreshToken: true`; Codex owns OAuth, token refresh and credential writes, and every network request for the signed-in account except the spending GET above and the term GET below. Limits receives identity metadata from the account subsystem before and after the handshake, rejecting a changed user or workspace. That subsystem resolves native file/keyring/auto storage and returns only the user/workspace observation key; no other credential bytes leave it, and the access token leaves it only through `codex_store::metadata_and_access`, for the spending and term GETs. The separate subscription-date reader also decodes `tokens.id_token` for account-matched `chatgpt_subscription_active_until` and `chatgpt_subscription_last_checked` claims. A future date is labeled cached Paid through, never renewal or cancellation; elapsed dates are unavailable. Multi-bucket app-server results and remembered per-account snapshots use canonical window ids, durations, reset instants, and per-window observation times. Codex reports some buckets no surface shows: its internal `base_model_inference` and `codex_bengalfox` buckets, matched by the id the reader gives their windows (`extra:<bucket>` or `extra:<bucket>:<slot>`), and the reserve and the retired Spark preview, matched by the model name after a label's last `·` (`gpt-reserve`, `gpt-5.3-codex-spark`, trimmed, in any case). The reader (`limits/codex.rs`) drops them as it parses, for signed-in and saved reads alike, and loading a remembered Codex snapshot drops them from files written before it did; so the cards, the account list, the limits monitor and both notches only ever see the windows a surface shows. Recent session `token_count.rate_limits` events can advance only a remembered account, and only when window id/kind, duration, and reset instant (within two seconds) identify exactly one quota window; ambiguous observations are ignored. The signed-in account's usage read has no fallback to the private ChatGPT usage endpoint; the spending and term GETs are the only ChatGPT backend requests on-n-off makes for it. The same response carries `rateLimitUpsell`, a banner the backend owns entirely: the app-server forwards it as untyped JSON (its nested keys stay snake_case) or nulls it when the account does not match the active login. on-n-off reads exactly one thing out of it — the call to action whose `action` is `buy_reset` — and shows it as a **Paid reset** row with the price, when the banner names one under `price.{amount_minor_units,currency}`. A currency is read only as three ISO 4217 letters and an amount only as a whole count of minor units within a sane bound; anything else leaves the offer priceless rather than the row absent. Nothing is ever bought, linked or opened: the purchase lives on the provider's own site. The offer is never written to a snapshot, never merged from a remembered read and never counts as an observation, because it is withdrawn the moment the account is under its limit again. **verified** (2026-09, openai/codex at `rust-v0.154.0` and the installed binaries' own generated bindings): `rate_limit_upsell` is `Option<serde_json::Value>` on the app-server response, gated all-or-nothing by an account match, with an in-repo test pinning verbatim passthrough; **code**: the `buy_reset` action and the `price` object, which the ChatGPT desktop app parses from the same backend payload, have not been observed on this machine. |
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

Claude plan labels come from the `oauthAccount` record every Claude usage read already checks
(`limits/claude_config.rs`): `organizationType` names the subscription as `claude_<type>`, and
`claude_max` at `organizationRateLimitTier=default_claude_max_5x` displays **Max ×5**, at
`default_claude_max_20x` **Max ×20**. Missing or unknown tiers keep **Max**; other subscription
types keep their own label. This requires no request of its own.
The selected account uses the existing green status dot beside the main usage-window label
(or in the card header when usage is unavailable).

A Claude card shows no subscription status, term or renewal date, and offers Archive account only
from its menu, never in place of Use account. Claude Code's usage report carries none of them, and
on-n-off asks Anthropic for nothing beside it. Until 2026-09-29 a status badge showed
`organization.subscription_status` from the profile read on-n-off then made itself; on the user's
decision (2026-09-29) the badge went with that read rather than keep a profile call. A remembered
reading written with a `subscriptionStatus` still loads, the field ignored. No renewal or expiry date
was available to the Claude Code OAuth token either: the profile carried only `subscription_status`
and `subscription_created_at`, and the billing page's own `subscription_details` read answered that
token with 404 on `/api/oauth/organizations/{org}/…` and 403 on `/api/organizations/{org}/…`.
**verified** (2026-09-24, one Max account, read-only probe); **code** (Claude Code 2.1.282's profile
mapper reads no renewal, expiry or cancellation field).

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
capability checks. Behind that gate, everything the account controls do differently per provider
lives in the provider's accounts adapter, `accounts/claude.rs` or `accounts/codex.rs`: which native
store the environment selects and when account changes defer to the official client, the CLI's
sign-in, sign-out and verification commands and their environment, the read and write of its login,
an isolated sign-in and its cleanup, which running processes are its clients, and how a saved
login is read (identity, email, credential generation, renewal). Save current, add, sign in again,
use, rename, remove, and sign out are separate operations. Add uses official CLI sign-in in an isolated home. Before saving, it attempts a first
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
ranks keep the backend's order, newest observation first. The rule lives in the card model both
surfaces render, `ui/src/features/limits/limitCards.ts`, on the shared `usageLeft` /
`usableAgainAt` / `planMultiplier` helpers; the backend itself still hands accounts over newest
first.

Native Codex file, keyring, and auto storage are handled explicitly. Ephemeral or alternate
credential backends, selected Codex configuration profiles, custom native homes (a Claude home
chosen by `CLAUDE_CONFIG_DIR`, or a store `CLAUDE_SECURESTORAGE_CONFIG_DIR` moved), environment auth
and detected forced-login policies are refused with guidance to use the official CLI. Claude's
isolated login uses the custom-home Keychain namespace on macOS; activation preserves shared MCP
OAuth and all unrelated configuration fields. Ordinary Claude activation allows running clients,
relying on Claude Code's native credential-change handling while retaining native refresh locks
(plus the config file's lock and, around the credential write, `.storage-write.lock`), the
protected journal and identity readback. Verification runs only after those locks are released.
Claude's verification is local (`ClaudeNative::verify`): it reads and identifies the published login,
runs `claude auth status --json`, which reads only what is stored and asks Anthropic nothing, and
requires `loggedIn: true`, the login's organization as `orgId` and, when the login names an email,
that email as `email`; then it reads the store again and requires the same login. It never renews:
an expired incoming login is accepted and Claude Code renews it on its next run, and a login
Anthropic refuses shows as a failed read at the next Limits poll. A local check rather than a
`/usage` run is the user's decision (2026-09-29). Codex activation still requires closed clients.
Sign-out and explicit crash recovery require closed clients for both providers. Existing IDE and
desktop sessions are not promised immediate adoption.

Saved credentials and interrupted-switch recovery live in an encrypted vault under
`~/.on-n-off/accounts/`; the vault key is in macOS Keychain or Windows Credential Manager. No
plaintext fallback exists for the vault. Active native credentials are authoritative, and inactive
profiles are not independently renewed by on-n-off after native activation. A saved Claude account
that is not the signed-in one keeps its login in its own home, a persistent Claude config dir under
`~/.on-n-off/accounts/homes/` (`accounts/homes.rs`, `accounts/claude/home.rs`; see the Homes
section of `docs/architecture/accounts.md`): the scoped Keychain entry `Claude Code-credentials-<hash
of that dir>` on macOS, created by on-n-off's first write there through `/usr/bin/security`, and the
home's `.credentials.json` on Windows, as Claude Code keeps any login there. Before every read of
saved accounts, such a login moves from the vault into its home; a switch moves it back out, and
empties the home, before publishing it. Its usage is Claude Code's own report in the home
(`limits/claude_cli.rs`, the Usage / Limits row above), and Claude Code renews the login there itself,
so on-n-off sends no Claude grant for a saved account. When Claude Code reports nothing, its `claude auth status --json`
in the home tells a home signed out of its login (`loggedIn: false`, the card asking for a new
sign-in) from a report that could not be had. A Claude login still in the vault, one whose move
failed, is never sent: its read fails with "This account's login has not moved into its home yet; the
next read tries again.", and the next read tries the move again. **verified** (2026-09-29, Claude Code 2.1.284, throwaway sign-ins):
Claude Code reads a home on-n-off built (only `oauthAccount` in `.claude.json`, the login filed by
`security`) with no onboarding, and a home emptied to Claude Code's signed-out shape answers with no
report. Claude Code's own logout deletes the home's Keychain entry; whether it also ends the
account's other logins is unproven, so removing a home never logs out. Saved-account usage polling
reads Codex with access tokens through its Limits reader, which the accounts adapter dispatches
(`Adapter::read_usage`); only never-activated isolated Codex sign-ins own automatic vault renewal
(`Adapter::renews_privately`). Saved Codex profiles use the account-scoped ChatGPT
usage endpoint (`wham/usage`, read beside app-server's parser in `limits/codex.rs`) separately from
the native app-server reader, which is never started for a saved profile; a body without an
`account_id` is accepted. A login that now signs in as a different account renews nothing and waits
for a new login: "This saved login now signs in as a different account. Sign in again." A rate-limited
saved read waits at least its `Retry-After`. Every card a saved poll produces is marked
`savedProfile`, so the UI says "Remembered account" only for readings that are neither the signed-in
account's nor a polled saved profile's. The first usage after a sign-in is read in the isolated
home by the provider's own client: Codex's app-server, and for Claude the report the signed-in card
reads (the Usage / Limits row above), `claude -p /usage --safe-mode` with the same arguments and
environment and `CLAUDE_CONFIG_DIR` set to that home (`limits/claude_cli.rs`). A saved account's home
and a sign-in's alike must name the expected user and organization in their `.claude.json`
`oauthAccount` before Claude Code starts and after it answers, and the plan comes from that record.
Any other missing report, a failed exit or no answer within 90 seconds reads as unavailable, never as
a refused login.
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

Account reads use `/usr/bin/security` (`accounts/keychain.rs`), and find Claude Code's item under
Claude Code's own account name first, then under the account a service-only lookup names (the
Account store row above). Limits' own Claude read opens no Keychain item: Claude Code reads its
login itself. The account list and saved-account polls do read the native store, to tell the
signed-in account apart. Each native verification still rereads the credential; no native login is cached
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
