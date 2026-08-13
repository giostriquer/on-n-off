export type AgentId = "claude" | "codex";

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
};

export type PluginDto = {
  id: string;
  name: string;
  source: string;
  enabled: boolean;
  skills: SkillDto[];
};

export type AgentTabDto = {
  plugins: PluginDto[];
  userSkills: SkillDto[];
};
