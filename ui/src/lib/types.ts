export type AgentId = "claude" | "codex" | "antigravity" | "cursor";

export type ErrorKind = "cli_missing" | "cli_too_old" | "parse" | "write" | "message";

export type AdapterError = {
  kind: ErrorKind;
  message: string;
  path: string | null;
};

export type AgentInfo = {
  id: AgentId;
  displayName: string;
  cliOk: boolean;
  cliError: string | null;
  installGit: boolean;
  installFolder: boolean;
  pluginToggle: boolean;
  /**
   * Whether on-n-off reads this provider's hooks at all. `false` gets an empty Hooks screen that
   * says so, because "no hooks" and "we never looked" are different facts — and no rail count,
   * because there is no number to be right about.
   */
  readsHooks: boolean;
};

export type SkillDto = {
  id: string;
  pluginId: string | null;
  name: string;
  description: string;
  enabled: boolean;
  togglable: boolean;
  origin?: string;
};

export type PluginDto = {
  id: string;
  name: string;
  source: string;
  version: string;
  upstream: string;
  outOfSync?: boolean;
  enabled: boolean;
  togglable: boolean;
  skills: SkillDto[];
};

/**
 * Where a server is configured: "" the user's own list, "project" the selected project,
 * "plugin" an enabled plugin, "local" Claude's list for particular projects (all-projects view).
 */
export type McpOrigin = "" | "project" | "plugin" | "local";

export type McpServerDto = {
  id: string;
  name: string;
  system: string;
  source: string;
  enabled: boolean;
  togglable: boolean;
  origin?: McpOrigin;
  /** The plugin (`name@marketplace`) that brings a "plugin" server. */
  pluginId?: string | null;
  /** The projects, as the provider keys them, that keep a "local" server. */
  projects?: string[];
};

/**
 * One hook handler a provider would run, as the backend found it. Read-only: on-n-off lists
 * these, never runs or edits them. `event` is the provider's own vocabulary (Claude's
 * `PreToolUse`, Codex's `notification`) and stays opaque here, and `command` stays unexpanded —
 * `${CLAUDE_PLUGIN_ROOT}/…` is what the user would see in their own settings file.
 */
export type HookDto = {
  id: string;
  event: string;
  /** "" when the entry has none, which for most events means "every tool". */
  matcher: string;
  /** "command" | "mcp_tool" | "http" | "prompt" | "agent", or whatever a newer CLI adds. */
  handler: string;
  /** The command line, `<server> · <tool>` for an mcp_tool handler, or "" when there is none. */
  command: string;
  /** Where it comes from: a plugin's display name, or the settings file that holds it. */
  source: string;
  pluginId: string | null;
  description: string;
  /** Codex's `[hooks.state]` switch. Claude has none, so its entries are always on. */
  enabled: boolean;
};

export type ProjectDto = {
  id: string;
  label: string;
  path: string;
  branch?: string;
  skillCount?: number;
  mcpCount?: number;
};

export type AgentTabDto = {
  plugins: PluginDto[];
  userSkills: SkillDto[];
  mcpServers: McpServerDto[];
  /** Absent on a tab serialized before hooks existed; read it through `filterTab`/`catalogCounts`. */
  hooks?: HookDto[];
};

export type FeatureFlags = {
  masterCut: boolean;
};

export type UpdaterBuildInfo = {
  enabled: boolean;
  installerKind: "nsis" | "dmg" | null;
  target: string | null;
};

export type LimitsPollMinutes = 5 | 10 | 15 | 30;
export type GithubPollSeconds = 30 | 60 | 120 | 300;

export type AppSettings = {
  hiddenAgents: AgentId[];
  binaryPaths: Partial<Record<AgentId, string>>;
  automaticUpdates: boolean;
  limitNotifications: boolean;
  limitsPollMinutes: LimitsPollMinutes;
  /** GitHub search qualifiers (`org:NAME`, `user:NAME`, `repo:OWNER/NAME`) narrowing "Mine". */
  githubScopes: string[];
  githubNotifications: boolean;
  githubPollSeconds: GithubPollSeconds;
  /** Windows only: closing the main window hides it into the tray instead of quitting. */
  closeToTray: boolean;
};

export type DiagnoseCheck = {
  id: string;
  label: string;
  ok: boolean;
  detail: string;
  hint?: string | null;
};

export type ProviderDiagnose = {
  agentId: AgentId;
  binary: string;
  homePath: string;
  checks: DiagnoseCheck[];
};

// --- Local items: skills/agents copied out of a marketplace by on-n-off ---------------------

export type ItemKind = "skill" | "agent";

export type ItemScope = { kind: "global" } | { kind: "project"; projectPath: string };

export type ItemSource = { owner: string; repo: string; ref: string };

/** How sure the backend's prose scan is that one entry needs another. */
export type DepConfidence = "high" | "medium";

/** Another marketplace entry that an entry names in its text. */
export type ItemDependency = {
  pluginName: string;
  kind: ItemKind;
  path: string;
  name: string;
  confidence: DepConfidence;
};

export type MarketplaceEntry = {
  name: string;
  description: string;
  path: string;
  dependsOn: ItemDependency[];
  /** Paths the text refers to that a local copy will not contain. */
  externalRefs: string[];
  /** The text mentions `CLAUDE_PLUGIN_ROOT`, so it expects to run inside the plugin. */
  usesPluginRoot: boolean;
};

/** Plugin-level assets a local copy never gets. */
export type PluginExtra = "commands" | "hooks" | "mcp";

export type MarketplacePlugin = {
  name: string;
  version: string | null;
  description: string;
  supported: boolean;
  source: ItemSource | null;
  skills: MarketplaceEntry[];
  agents: MarketplaceEntry[];
  extras: PluginExtra[];
};

export type MarketplaceInspect = {
  isMarketplace: boolean;
  commitSha: string;
  marketplaceName: string;
  plugins: MarketplacePlugin[];
  hint: string | null;
};

export type ItemPick = { pluginName: string; kind: ItemKind; path: string; source: ItemSource | null };

export type ItemTarget = { provider: AgentId; scope: ItemScope };

export type InstallItemsRequest = {
  source: ItemSource;
  commitSha: string;
  items: ItemPick[];
  targets: ItemTarget[];
  overwriteUnmanaged: boolean;
};

export type ItemOutcomeStatus = "installed" | "replaced" | "skipped" | "conflict" | "failed";

export type ItemOutcome = {
  provider: AgentId;
  kind: ItemKind;
  name: string;
  /** The pick this outcome answers. */
  pluginName: string;
  path: string;
  targetPath: string;
  status: ItemOutcomeStatus;
  reason: string | null;
};

export type InstallItemsResult = { commitSha: string; shaMoved: boolean; outcomes: ItemOutcome[] };

export type ItemUpstream =
  | { state: "unknown" }
  | { state: "current" }
  | { state: "updateAvailable"; commitSha: string; pluginVersion: string | null };

export type ItemStatus = {
  id: string;
  provider: AgentId;
  kind: ItemKind;
  name: string;
  displayName: string;
  targetPath: string;
  installedVersion: string | null;
  installedSha: string;
  modified: boolean;
  missing: boolean;
  upstream: ItemUpstream;
  /** Where the item was copied from, so the UI can say so and link to it. */
  source: ItemSource;
  pluginName: string;
  upstreamPath: string;
  /** GitHub page of the item at the installed commit. */
  upstreamUrl: string;
};

export type UpdateItemMode = "overwrite" | "dismiss";

/**
 * A read the backend caches once for every surface that wants it. The screens, the menu-bar
 * popovers, the notch and the monitors share these, so any of them can be the one that fetches;
 * the announcement is how the rest find out. These strings are the contract with `Source::name`
 * in `src-tauri/src/read_revision.rs`; change the two together.
 */
export type SharedReadSource = "accounts" | "limits:claude" | "limits:codex" | "github:prs";

/** Sent once per replacement of a shared read, never for a read that changed nothing. */
export type SharedReadChanged = {
  source: SharedReadSource;
};
