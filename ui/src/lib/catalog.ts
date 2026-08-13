import { copy } from "./copy";
import { isProjectOrigin } from "./project";
import type { AgentId, AgentTabDto, McpServerDto, PluginDto, SkillDto } from "./types";

export type Screen = "overview" | "plugins" | "skills" | "mcp" | "usage" | "config";

export type KindCounts = { on: number; total: number };

export type CatalogCounts = {
  plugins: KindCounts;
  skills: KindCounts;
  mcp: KindCounts;
};

export type LiveRow = {
  kind: "plugin" | "skill" | "mcp";
  id: string;
  name: string;
  meta: string;
  enabled: boolean;
  togglable: boolean;
};

export type DriftRow = {
  kind: "plugin";
  id: string;
  name: string;
  version: string;
  upstream: string;
};

export function comparePluginThenName(
  aPlugin: string,
  aName: string,
  bPlugin: string,
  bName: string,
): number {
  const plugin = aPlugin.localeCompare(bPlugin, undefined, { sensitivity: "accent" });
  if (plugin !== 0) {
    return plugin;
  }
  const name = aName.localeCompare(bName, undefined, { sensitivity: "accent" });
  if (name !== 0) {
    return name;
  }
  return aPlugin.localeCompare(bPlugin) || aName.localeCompare(bName);
}

export function pluginNameForSkill(skill: SkillDto, plugins: PluginDto[] = []): string {
  if (!skill.pluginId) {
    return "";
  }
  return plugins.find((plugin) => plugin.id === skill.pluginId)?.name ?? skill.pluginId;
}

function versionKey(value: string): string {
  return value.trim().replace(/^v/i, "").toLowerCase();
}

function isCommitSha(value: string): boolean {
  return /^[a-f0-9]{7,40}$/i.test(value.trim());
}

export function canUninstallPlugin(plugin: PluginDto): boolean {
  if (plugin.id.startsWith("project:")) {
    return false;
  }
  const source = plugin.source.toLowerCase();
  return source !== "config" && source !== "workspace";
}

export function pluginOutOfSync(plugin: PluginDto): boolean {
  if (typeof plugin.outOfSync === "boolean") {
    return plugin.outOfSync;
  }
  const installed = plugin.version.trim();
  const upstream = plugin.upstream.trim();
  if (
    !installed ||
    !upstream ||
    installed.toLowerCase() === "unknown" ||
    upstream.toLowerCase() === "unknown" ||
    isCommitSha(installed) ||
    isCommitSha(upstream)
  ) {
    return false;
  }
  return versionKey(installed) !== versionKey(upstream);
}

export function formatPluginVersion(version: string): string {
  const value = version.trim();
  if (!value) {
    return "";
  }
  if (/^v/i.test(value) || !/^\d/.test(value)) {
    return value;
  }
  return `v${value}`;
}

export function pluginVersionNote(plugin: PluginDto): string {
  const upstream = isCommitSha(plugin.upstream) ? "" : plugin.upstream.trim();
  const version = isCommitSha(plugin.version) ? "" : plugin.version.trim();
  if (!upstream) {
    return version ? "installed" : "";
  }
  if (pluginOutOfSync({ ...plugin, version, upstream })) {
    return `upstream ${formatPluginVersion(upstream)}`;
  }
  return version ? copy.upToDate : "";
}

export function driftRows(tab: AgentTabDto): DriftRow[] {
  return sortPlugins(tab.plugins.filter(pluginOutOfSync)).map((plugin) => ({
    kind: "plugin",
    id: plugin.id,
    name: plugin.name,
    version: plugin.version,
    upstream: plugin.upstream,
  }));
}

export function driftLine(count: number): string {
  return count === 1 ? "1 plugin behind catalog" : `${count} plugins behind catalog`;
}

export function sortPlugins(plugins: PluginDto[]): PluginDto[] {
  return [...plugins]
    .map((plugin) => ({ ...plugin, skills: sortSkills(plugin.skills, [plugin]) }))
    .sort((a, b) => comparePluginThenName(a.source, a.name, b.source, b.name));
}

export function sortSkills(skills: SkillDto[], plugins: PluginDto[] = []): SkillDto[] {
  return [...skills].sort((a, b) =>
    comparePluginThenName(pluginNameForSkill(a, plugins), a.name, pluginNameForSkill(b, plugins), b.name),
  );
}

export function sortMcps(servers: McpServerDto[]): McpServerDto[] {
  return [...servers].sort((a, b) => comparePluginThenName(a.name, a.system, b.name, b.system));
}

export function allSkills(tab: AgentTabDto): SkillDto[] {
  return sortSkills([...tab.plugins.flatMap((plugin) => plugin.skills), ...tab.userSkills], tab.plugins);
}

export function skillIsLive(skill: SkillDto, tab: AgentTabDto): boolean {
  if (skill.togglable) {
    return skill.enabled;
  }
  if (!skill.pluginId) {
    return skill.enabled;
  }
  return tab.plugins.find((plugin) => plugin.id === skill.pluginId)?.enabled ?? skill.enabled;
}

export function catalogCounts(tab: AgentTabDto | null): CatalogCounts {
  if (!tab) {
    return {
      plugins: { on: 0, total: 0 },
      skills: { on: 0, total: 0 },
      mcp: { on: 0, total: 0 },
    };
  }
  const skills = allSkills(tab);
  const mcps = tab.mcpServers ?? [];
  return {
    plugins: {
      on: tab.plugins.filter((plugin) => plugin.enabled).length,
      total: tab.plugins.length,
    },
    skills: {
      on: skills.filter((skill) => skillIsLive(skill, tab)).length,
      total: skills.length,
    },
    mcp: {
      on: mcps.filter((server) => server.enabled).length,
      total: mcps.length,
    },
  };
}

export function liveRows(tab: AgentTabDto): LiveRow[] {
  const rows: LiveRow[] = [];
  for (const plugin of tab.plugins) {
    if (plugin.enabled) {
      rows.push({
        kind: "plugin",
        id: plugin.id,
        name: plugin.name,
        meta: plugin.version
          ? `plugin · ${plugin.source} · ${plugin.version}`
          : `plugin · ${plugin.source}`,
        enabled: true,
        togglable: plugin.togglable,
      });
    }
    for (const skill of plugin.skills) {
      if (!skillIsLive(skill, tab)) {
        continue;
      }
      rows.push({
        kind: "skill",
        id: skill.id,
        name: skill.name,
        meta: skill.togglable ? "skill · user" : `skill · ${plugin.name}`,
        enabled: true,
        togglable: skill.togglable,
      });
    }
  }
  for (const skill of tab.userSkills) {
    if (!skill.enabled) {
      continue;
    }
    rows.push({
      kind: "skill",
      id: skill.id,
      name: skill.name,
      meta: isProjectOrigin(skill.origin) ? "skill · project" : "skill · user",
      enabled: true,
      togglable: skill.togglable,
    });
  }
  for (const server of tab.mcpServers ?? []) {
    if (!server.enabled) {
      continue;
    }
    rows.push({
      kind: "mcp",
      id: server.id,
      name: server.name,
      meta: `mcp · ${server.source || server.system}`,
      enabled: true,
      togglable: server.togglable,
    });
  }
  return rows.sort((a, b) => comparePluginThenName(livePlugin(a), a.name, livePlugin(b), b.name));
}

function livePlugin(row: LiveRow): string {
  if (row.kind === "plugin") {
    return row.name;
  }
  if (row.kind === "skill") {
    const plugin = row.meta.replace(/^skill · /, "");
    return plugin === "user" ? "" : plugin;
  }
  return "";
}

export function tallyLine(counts: CatalogCounts, agentName: string): string {
  return `${counts.plugins.on} plugins · ${counts.skills.on} skills · ${counts.mcp.on} mcps live on ${agentName}`;
}

export function masterAllOn(tab: AgentTabDto | null): boolean {
  if (!tab) {
    return false;
  }
  const plugins = tab.plugins.filter((plugin) => plugin.togglable);
  const skills = tab.userSkills.filter((skill) => skill.togglable);
  const mcps = (tab.mcpServers ?? []).filter((server) => server.togglable);
  if (plugins.length === 0 && skills.length === 0 && mcps.length === 0) {
    return false;
  }
  return (
    plugins.every((plugin) => plugin.enabled) &&
    skills.every((skill) => skill.enabled) &&
    mcps.every((server) => server.enabled)
  );
}

export function agentRoot(agentId: AgentId): string {
  switch (agentId) {
    case "claude":
      return "~/.claude";
    case "codex":
      return "~/.codex";
    case "antigravity":
      return "~/.gemini";
  }
}

export function mcpConfigPath(agentId: AgentId): string {
  switch (agentId) {
    case "claude":
      return "~/.claude.json";
    case "codex":
      return "~/.codex/config.toml";
    case "antigravity":
      return "~/.gemini/config/mcp_config.json";
  }
}

export function scopeConfigPath(agentId: AgentId): string {
  switch (agentId) {
    case "claude":
      return "~/.claude/settings.json";
    case "codex":
      return "~/.codex/config.toml";
    case "antigravity":
      return "~/.gemini/antigravity-cli/";
  }
}

export function globalItemCount(tab: AgentTabDto | null | undefined): number {
  if (!tab) {
    return 0;
  }
  const project = (origin?: string) => origin?.toLowerCase() === "project";
  return (
    tab.plugins.length +
    tab.userSkills.filter((skill) => !project(skill.origin)).length +
    (tab.mcpServers ?? []).filter((server) => !project(server.origin)).length
  );
}

export function emptyTabDto(): AgentTabDto {
  return { plugins: [], userSkills: [], mcpServers: [] };
}
