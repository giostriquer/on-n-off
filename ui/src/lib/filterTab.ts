import { allSkills, sortHooks, sortMcps, sortPlugins, sortSkills } from "./catalog";
import type { AgentTabDto, HookDto, McpServerDto, PluginDto, SkillDto } from "./types";

function matches(query: string, ...parts: string[]): boolean {
  return parts.join(" ").toLowerCase().includes(query);
}

/** A server by its name, id, transport, source, or the plugin or project it comes from. */
function matchesMcp(query: string, server: McpServerDto): boolean {
  return matches(query, server.name, server.id, server.system, server.source, server.via ?? "");
}

export type FilteredTab = {
  plugins: PluginDto[];
  skills: SkillDto[];
  mcpServers: McpServerDto[];
  hooks: HookDto[];
  expandIds: string[];
};

export function filterTab(tab: AgentTabDto, query: string): FilteredTab {
  const q = query.trim().toLowerCase();
  const mcpServers = tab.mcpServers ?? [];
  const skills = filterSkillList(tab, q);
  const hooks = filterHookList(tab, q);
  if (!q) {
    return {
      plugins: sortPlugins(tab.plugins),
      skills,
      mcpServers: sortMcps(mcpServers),
      hooks,
      expandIds: [],
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

  const filteredMcps = sortMcps(
    mcpServers.filter((server) => matchesMcp(q, server)),
  );

  return {
    plugins,
    skills,
    mcpServers: filteredMcps,
    hooks,
    expandIds,
  };
}

/**
 * Hooks match on everything the row shows, plus the plugin that brought them. The id stays out:
 * it is the backend's key, `<plugin>:<source>:<event>:<group>:<index>`, so searching it would let
 * a digit or a snake_cased event match rows that show neither.
 */
function filterHookList(tab: AgentTabDto, query: string): HookDto[] {
  const hooks = sortHooks(tab.hooks ?? []);
  if (!query) {
    return hooks;
  }
  return hooks.filter((hook) =>
    matches(
      query,
      hook.event,
      hook.matcher,
      hook.handler,
      hook.command,
      hook.source,
      hook.description,
      hook.pluginId ?? "",
    ),
  );
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
  return servers.filter((server) => matchesMcp(q, server));
}
