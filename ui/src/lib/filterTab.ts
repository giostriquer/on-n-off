import type { AgentTabDto, PluginDto, SkillDto } from "./types";

function matches(query: string, ...parts: string[]): boolean {
  return parts.join(" ").toLowerCase().includes(query);
}

export type FilteredTab = {
  plugins: PluginDto[];
  userSkills: SkillDto[];
  expandIds: string[];
  emptyBecauseFilter: boolean;
};

export function filterTab(tab: AgentTabDto, query: string): FilteredTab {
  const q = query.trim().toLowerCase();
  if (!q) {
    return {
      plugins: tab.plugins,
      userSkills: tab.userSkills,
      expandIds: [],
      emptyBecauseFilter: false,
    };
  }

  const expandIds: string[] = [];
  const plugins = tab.plugins.filter((plugin) => {
    const pluginHit = matches(q, plugin.name, plugin.id, plugin.source);
    const skillHit = plugin.skills.some((skill) =>
      matches(q, skill.name, skill.id, skill.description),
    );
    if (skillHit) {
      expandIds.push(plugin.id);
    }
    return pluginHit || skillHit;
  });

  const userSkills = tab.userSkills.filter((skill) =>
    matches(q, skill.name, skill.id, skill.description),
  );

  return {
    plugins,
    userSkills,
    expandIds,
    emptyBecauseFilter: plugins.length === 0 && userSkills.length === 0,
  };
}
