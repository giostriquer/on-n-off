import type { AgentId, AppSettings } from "./types";

export const ALL_AGENTS: readonly AgentId[] = ["claude", "codex", "antigravity"];

export const DEFAULT_APP_SETTINGS: AppSettings = {
  hiddenAgents: [],
  binaryPaths: {},
};

export function mergeAppSettings(overlay: Partial<AppSettings> | null | undefined): AppSettings {
  return {
    hiddenAgents: overlay?.hiddenAgents ?? [],
    binaryPaths: overlay?.binaryPaths ?? {},
  };
}

export function visibleAgentIds(hidden: readonly AgentId[]): AgentId[] {
  const set = new Set(hidden);
  const visible = ALL_AGENTS.filter((id) => !set.has(id));
  return visible.length > 0 ? [...visible] : ["claude"];
}

/** Returns the next hidden list. Refuses to hide the last visible provider. */
export function setAgentHidden(hidden: readonly AgentId[], id: AgentId, hide: boolean): AgentId[] {
  const next = new Set(hidden);
  if (hide) {
    next.add(id);
    if (ALL_AGENTS.every((agent) => next.has(agent))) {
      return ALL_AGENTS.filter((agent) => hidden.includes(agent));
    }
  } else {
    next.delete(id);
  }
  return ALL_AGENTS.filter((agent) => next.has(agent));
}
