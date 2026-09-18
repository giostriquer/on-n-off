import type { AgentId, HookDto } from "$lib/types";

/**
 * Synthetic hooks for the UI harness (`?mock=hooks`). Invented throughout — acme/webapp names, no
 * real path, plugin or address — and shaped to cover the cases the screen has to get right: a
 * plugin hook carrying a description, a plain settings-file hook, an `mcp_tool` handler, a handler
 * with no command at all, a command long enough to need truncating, and a Codex entry switched off
 * in `[hooks.state]`. Claude's and Codex's event names come from different vocabularies on
 * purpose: the screen prints whatever the provider calls it. Ids follow the backend's shape,
 * `<plugin-id>:<source>:<event>:<group>:<index>`, with an empty plugin segment for user settings.
 */

const CLAUDE: HookDto[] = [
  {
    id: ":settings.json:pre_tool_use:0:0",
    event: "PreToolUse",
    matcher: "Bash",
    handler: "command",
    command: "~/.claude/hooks/guard-bash.sh --deny 'rm -rf' --deny 'curl | sh' --log ~/.claude/hooks/guard.log",
    source: "settings.json",
    pluginId: null,
    description: "",
    enabled: true,
  },
  {
    id: ":settings.json:stop:0:0",
    event: "Stop",
    matcher: "",
    handler: "command",
    command: "~/.claude/hooks/say-done.sh",
    source: "settings.json",
    pluginId: null,
    description: "",
    enabled: true,
  },
  {
    id: "acme-guardrails@webapp:hooks.json:pre_tool_use:0:0",
    event: "PreToolUse",
    matcher: "Write|Edit",
    handler: "command",
    command: "${CLAUDE_PLUGIN_ROOT}/bin/check-secrets.js --strict --allow ${CLAUDE_PROJECT_DIR}/fixtures",
    source: "acme-guardrails",
    pluginId: "acme-guardrails@webapp",
    description: "Refuses a write that would put a credential in the repository.",
    enabled: true,
  },
  {
    id: "acme-guardrails@webapp:hooks.json:post_tool_use:0:0",
    event: "PostToolUse",
    matcher: "",
    handler: "mcp_tool",
    command: "acme-review · lint_diff",
    source: "acme-guardrails",
    pluginId: "acme-guardrails@webapp",
    description: "Lints every diff the agent writes, through the acme-review MCP server.",
    enabled: true,
  },
  {
    id: "webapp-house-rules@webapp:hooks.json:session_start:0:0",
    event: "SessionStart",
    matcher: "",
    handler: "prompt",
    command: "",
    source: "webapp-house-rules",
    pluginId: "webapp-house-rules@webapp",
    description: "Reads the webapp house rules into the session before the first turn.",
    enabled: true,
  },
];

const CODEX: HookDto[] = [
  {
    id: ":hooks.json:session_start:0:0",
    event: "SessionStart",
    matcher: "",
    handler: "command",
    command: "~/.codex/hooks/log-session.sh --dir ~/.codex/hooks/sessions",
    source: "hooks.json",
    pluginId: null,
    description: "",
    enabled: true,
  },
  {
    id: ":config.toml:pre_tool_use:0:0",
    event: "PreToolUse",
    matcher: "shell",
    handler: "command",
    command: "~/.codex/hooks/audit-shell.sh --json",
    source: "config.toml",
    pluginId: null,
    description: "",
    enabled: true,
  },
  {
    id: ":config.toml:notify:0:0",
    event: "Notification",
    matcher: "",
    handler: "command",
    command: "~/.codex/notify.py",
    source: "config.toml · legacy notify",
    pluginId: null,
    description: "",
    enabled: true,
  },
  {
    id: "acme-webapp-tools@webapp:plugin.json:stop:0:0",
    event: "Stop",
    matcher: "",
    handler: "mcp_tool",
    command: "acme-review · summarize_task",
    source: "acme-webapp-tools",
    pluginId: "acme-webapp-tools@webapp",
    description: "Posts a summary of the finished task to the team channel.",
    enabled: false,
  },
];

export function hooksFor(agentId: AgentId): HookDto[] {
  if (agentId === "claude") {
    return CLAUDE;
  }
  // Antigravity and Cursor have none by design: the screen says on-n-off does not read theirs.
  return agentId === "codex" ? CODEX : [];
}
