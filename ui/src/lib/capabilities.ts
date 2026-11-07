import type {
  AgentId,
  AgentInfo,
  Capabilities,
  Change,
  InstallSourceKind,
  ItemPlace,
  ItemScope,
  PluginDto,
} from "./types";

const SOURCE_KINDS: readonly InstallSourceKind[] = ["gitMarketplace", "folder", "marketplacePlugin", "marketplaceSkill"];

const UNKNOWN: Change = { by: "refused", how: "", reason: "" };

export const NOTHING_OFFERED: Capabilities = {
  pluginSwitch: UNKNOWN,
  skillSwitch: UNKNOWN,
  mcpSwitch: UNKNOWN,
  refusedGlobally: {},
  install: UNKNOWN,
  refusedSources: {},
  uninstall: UNKNOWN,
  uninstallFrom: null,
  update: UNKNOWN,
  readsHooks: false,
};

export function ready(agent: AgentInfo, change: Change): boolean {
  return change.by === "file" || (change.by === "cli" && agent.cliOk);
}

export function canSwitchPlugins(agent: AgentInfo): boolean {
  return ready(agent, agent.capabilities.pluginSwitch);
}

export function sourceRefusal(agent: AgentInfo, kind: InstallSourceKind): string | null {
  return agent.capabilities.refusedSources[kind] ?? null;
}

export function installsFrom(agent: AgentInfo, kind: InstallSourceKind): boolean {
  return ready(agent, agent.capabilities.install) && sourceRefusal(agent, kind) === null;
}

export function canInstall(agent: AgentInfo): boolean {
  return SOURCE_KINDS.some((kind) => installsFrom(agent, kind));
}

export function canUninstallPlugin(agent: AgentInfo, plugin: PluginDto): boolean {
  if (plugin.id.startsWith("project:") || !ready(agent, agent.capabilities.uninstall)) {
    return false;
  }
  const from = agent.capabilities.uninstallFrom;
  return from === null || from.includes(plugin.source);
}

export function canUpdate(agent: AgentInfo): boolean {
  return ready(agent, agent.capabilities.update);
}

export function mcpNotice(agent: AgentInfo, scopePath: string | null): string | null {
  const change = agent.capabilities.mcpSwitch;
  if (change.by === "refused") {
    return change.reason || null;
  }
  return scopePath?.trim() ? null : (agent.capabilities.refusedGlobally.mcp ?? null);
}

export function chosenAgents(agents: readonly AgentInfo[], providers: readonly AgentId[]): AgentInfo[] {
  return providers.flatMap((id) => agents.filter((agent) => agent.id === id));
}

function placeFor(agent: AgentInfo, scope: ItemScope): ItemPlace | null {
  return scope.kind === "global" ? agent.itemRoots.global : agent.itemRoots.project;
}

export function holdsSubagents(agent: AgentInfo): boolean {
  return Boolean(agent.itemRoots.global?.agents || agent.itemRoots.project?.agents);
}

export function takesSubagents(agents: readonly AgentInfo[], providers: readonly AgentId[], scope: ItemScope): boolean {
  return chosenAgents(agents, providers).some((agent) => Boolean(placeFor(agent, scope)?.agents));
}

export function previewPath(agent: AgentInfo, scope: ItemScope): string {
  const skills = placeFor(agent, scope)?.skills ?? "";
  if (scope.kind === "global") {
    return skills;
  }
  return `${scope.projectPath.replace(/[\\/]+$/, "")}/${skills}`;
}
