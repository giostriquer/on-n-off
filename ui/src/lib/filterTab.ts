import { allSkills, sortMcps, sortPlugins, sortSkills } from "./catalog";
import type { AgentTabDto, McpServerDto, PluginDto, SkillDto } from "./types";

function matches(query: string, ...parts: string[]): boolean {
  return parts.join(" ").toLowerCase().includes(query);
}

export type FilteredTab = {
  plugins: PluginDto[];
  userSkills: SkillDto[];
  mcpServers: McpServerDto[];
  expandIds: string[];
  emptyBecauseFilter: boolean;
};

export function filterTab(tab: AgentTabDto, query: string): FilteredTab {
  const q = query.trim().toLowerCase();
  const mcpServers = tab.mcpServers ?? [];
  if (!q) {
    return {
      plugins: sortPlugins(tab.plugins),
      userSkills: sortSkills(tab.userSkills),
      mcpServers: sortMcps(mcpServers),
      expandIds: [],
      emptyBecauseFilter: false,
    };
  }

  const expandIds: string[] = [];
  const plugins = sortPlugins(
    tab.plugins.filter((plugin) => {
      const pluginHit = matches(q, plugin.name, plugin.id, plugin.source, plugin.version, plugin.upstream);
      const skillHit = plugin.skills.some((skill) =>
        matches(q, skill.name, skill.id, skill.description),
      );
      if (skillHit) {
        expandIds.push(plugin.id);
      }
      return pluginHit || skillHit;
    }),
  );

  const userSkills = sortSkills(
    tab.userSkills.filter((skill) => matches(q, skill.name, skill.id, skill.description)),
  );
  const filteredMcps = sortMcps(
    mcpServers.filter((server) => matches(q, server.name, server.id, server.system, server.source)),
  );

  return {
    plugins,
    userSkills,
    mcpServers: filteredMcps,
    expandIds,
    emptyBecauseFilter: plugins.length === 0 && userSkills.length === 0 && filteredMcps.length === 0,
  };
}

export function filterSkillList(tab: AgentTabDto, query: string): SkillDto[] {
  const q = query.trim().toLowerCase();
  const skills = allSkills(tab);
  if (!q) {
    return skills;
  }
  return sortSkills(
    skills.filter((skill) => matches(q, skill.name, skill.id, skill.description, skill.pluginId ?? "")),
    tab.plugins,
  );
}

export function filterMcpList(tab: AgentTabDto, query: string): McpServerDto[] {
  const q = query.trim().toLowerCase();
  const servers = sortMcps(tab.mcpServers ?? []);
  if (!q) {
    return servers;
  }
  return servers.filter((server) => matches(q, server.name, server.id, server.system, server.source));
}
