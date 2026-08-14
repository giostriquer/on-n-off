export type AgentId = "claude" | "codex" | "antigravity";

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

export type AppSettings = {
  hiddenAgents: AgentId[];
  binaryPaths: Partial<Record<AgentId, string>>;
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
