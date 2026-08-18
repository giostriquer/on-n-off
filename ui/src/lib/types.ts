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

export type MarketplaceEntry = { name: string; description: string; path: string };

export type MarketplacePlugin = {
  name: string;
  version: string | null;
  description: string;
  supported: boolean;
  source: ItemSource | null;
  skills: MarketplaceEntry[];
  agents: MarketplaceEntry[];
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
};

export type UpdateItemMode = "overwrite" | "dismiss";
