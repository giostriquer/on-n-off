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

export type McpServerDto = {
  id: string;
  name: string;
  system: string;
  source: string;
  enabled: boolean;
  togglable: boolean;
  origin?: string;
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
};

export type FeatureFlags = {
  masterCut: boolean;
};

export type UpdaterBuildInfo = {
  enabled: boolean;
  installerKind: "nsis" | "msi" | "dmg" | null;
  target: string | null;
};

export type AppSettings = {
  hiddenAgents: AgentId[];
  binaryPaths: Partial<Record<AgentId, string>>;
  automaticUpdates: boolean;
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
