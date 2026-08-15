import type { AgentId, AgentInfo, AgentTabDto } from "./types";

export type TabState = {
  dto: AgentTabDto | null;
  filter: string;
  expanded: Set<string>;
  loading: boolean;
  inFlight: boolean;
  error: string | null;
};

export const LOCKED_AGENTS: AgentInfo[] = [
  {
    id: "claude",
    displayName: "Claude",
    cliOk: false,
    cliError: null,
    installGit: false,
    installFolder: false,
    pluginToggle: false,
  },
  {
    id: "codex",
    displayName: "Codex",
    cliOk: false,
    cliError: null,
    installGit: false,
    installFolder: false,
    pluginToggle: false,
  },
  {
    id: "antigravity",
    displayName: "Antigravity",
    cliOk: false,
    cliError: null,
    installGit: false,
    installFolder: false,
    pluginToggle: false,
  },
  {
    id: "cursor",
    displayName: "Cursor",
    cliOk: false,
    cliError: null,
    installGit: false,
    installFolder: false,
    pluginToggle: false,
  },
];

export function emptyAgentRecord<T>(value: () => T): Record<AgentId, T> {
  return {
    claude: value(),
    codex: value(),
    antigravity: value(),
    cursor: value(),
  };
}

export function emptyTab(): TabState {
  return {
    dto: null,
    filter: "",
    expanded: new Set(),
    loading: true,
    inFlight: false,
    error: null,
  };
}

export function overlayAgents(health: AgentInfo[]): AgentInfo[] {
  return LOCKED_AGENTS.map((locked) => {
    const hit = health.find((agent) => agent.id === locked.id);
    if (!hit) {
      return {
        ...locked,
        cliOk: false,
        cliError: "Agent health unavailable.",
      };
    }
    return {
      ...locked,
      cliOk: hit.cliOk,
      cliError: hit.cliError,
      installGit: hit.installGit,
      installFolder: hit.installFolder,
      pluginToggle: hit.pluginToggle,
    };
  });
}

export function openIds(userExpanded: Set<string>, filterExpand: string[]): Set<string> {
  const next = new Set(userExpanded);
  for (const id of filterExpand) {
    next.add(id);
  }
  return next;
}

export function mergeEnrichedPluginMetadata(
  local: AgentTabDto,
  enriched: AgentTabDto,
): AgentTabDto {
  const enrichedById = new Map(enriched.plugins.map((plugin) => [plugin.id, plugin]));
  return {
    ...local,
    plugins: local.plugins.map((plugin) => {
      const metadata = enrichedById.get(plugin.id);
      if (!metadata) {
        return plugin;
      }
      return {
        ...plugin,
        version: metadata.version,
        upstream: metadata.upstream,
        outOfSync: metadata.outOfSync,
      };
    }),
  };
}

export async function withAgentLock<T>(
  tabs: Record<AgentId, TabState>,
  agentId: AgentId,
  fn: () => Promise<T>,
): Promise<T | undefined> {
  if (tabs[agentId].inFlight) {
    return undefined;
  }
  tabs[agentId].inFlight = true;
  try {
    return await fn();
  } finally {
    tabs[agentId].inFlight = false;
  }
}
