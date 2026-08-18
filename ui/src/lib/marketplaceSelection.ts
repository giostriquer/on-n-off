import { agentRoot } from "./catalog";
import type {
  AgentId,
  ItemKind,
  ItemPick,
  ItemScope,
  ItemTarget,
  MarketplaceInspect,
  MarketplacePlugin,
} from "./types";

export type MarketplaceAction = "plugin" | "all" | "selected";

const KEY_SEP = " ";

export function entryKey(pluginName: string, kind: ItemKind, path: string): string {
  return [pluginName, kind, path].join(KEY_SEP);
}

/** Every installable entry (agents included) of every supported plugin, in display order. */
export function allKeys(inspect: MarketplaceInspect): string[] {
  return inspect.plugins.flatMap((plugin) => [
    ...groupKeys(plugin, "skill"),
    ...groupKeys(plugin, "agent"),
  ]);
}

export function groupKeys(plugin: MarketplacePlugin, kind: ItemKind): string[] {
  if (!plugin.supported) {
    return [];
  }
  const entries = kind === "skill" ? plugin.skills : plugin.agents;
  return entries.map((entry) => entryKey(plugin.name, kind, entry.path));
}

export function toggleGroup(
  keys: ReadonlySet<string>,
  plugin: MarketplacePlugin,
  kind: ItemKind,
  on: boolean,
): Set<string> {
  const next = new Set(keys);
  for (const key of groupKeys(plugin, kind)) {
    if (on) {
      next.add(key);
    } else {
      next.delete(key);
    }
  }
  return next;
}

/** Picks for the backend, in marketplace order, carrying the plugin's own repo when it has one. */
export function selectedItems(inspect: MarketplaceInspect, keys: ReadonlySet<string>): ItemPick[] {
  const picks: ItemPick[] = [];
  for (const plugin of inspect.plugins) {
    if (!plugin.supported) {
      continue;
    }
    for (const skill of plugin.skills) {
      if (keys.has(entryKey(plugin.name, "skill", skill.path))) {
        picks.push({ pluginName: plugin.name, kind: "skill", path: skill.path, source: plugin.source });
      }
    }
    for (const agent of plugin.agents) {
      if (keys.has(entryKey(plugin.name, "agent", agent.path))) {
        picks.push({ pluginName: plugin.name, kind: "agent", path: agent.path, source: plugin.source });
      }
    }
  }
  return picks;
}

export function agentsAllowed(providers: readonly AgentId[]): boolean {
  return providers.includes("claude");
}

export function targetsFor(providers: readonly AgentId[], scope: ItemScope): ItemTarget[] {
  return providers.map((provider) => ({ provider, scope }));
}

/** Where a provider's skills land for a scope — a preview of `AgentAdapter::item_roots`. */
export function previewPath(provider: AgentId, scope: ItemScope): string {
  if (scope.kind === "project") {
    const base = scope.projectPath.replace(/[\\/]+$/, "");
    switch (provider) {
      case "claude":
        return `${base}/.claude/skills`;
      case "codex":
        return `${base}/.codex/skills`;
      case "antigravity":
        return `${base}/.agents/skills`;
      case "cursor":
        return `${base}/.cursor/skills`;
    }
  }
  return provider === "antigravity" ? "~/.gemini/antigravity-cli/skills" : `${agentRoot(provider)}/skills`;
}
