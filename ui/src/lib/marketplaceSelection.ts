import { agentRoot } from "./catalog";
import type {
  AgentId,
  InstallItemsResult,
  ItemKind,
  ItemOutcome,
  ItemPick,
  ItemScope,
  ItemTarget,
  MarketplaceInspect,
} from "./types";

export type MarketplaceAction = "plugin" | "all" | "selected";

const KEY_SEP = " ";

export function entryKey(pluginName: string, kind: ItemKind, path: string): string {
  return [pluginName, kind, path].join(KEY_SEP);
}

/** Every installable entry (agents included) of every supported plugin, in display order. */
export function allKeys(inspect: MarketplaceInspect): string[] {
  const keys: string[] = [];
  for (const plugin of inspect.plugins) {
    if (!plugin.supported) {
      continue;
    }
    for (const skill of plugin.skills) {
      keys.push(entryKey(plugin.name, "skill", skill.path));
    }
    for (const agent of plugin.agents) {
      keys.push(entryKey(plugin.name, "agent", agent.path));
    }
  }
  return keys;
}

export function groupKeys(inspect: MarketplaceInspect, pluginName: string, kind: ItemKind): string[] {
  const plugin = inspect.plugins.find((p) => p.name === pluginName);
  if (!plugin?.supported) {
    return [];
  }
  const entries = kind === "skill" ? plugin.skills : plugin.agents;
  return entries.map((entry) => entryKey(pluginName, kind, entry.path));
}

export function toggleGroup(
  keys: ReadonlySet<string>,
  inspect: MarketplaceInspect,
  pluginName: string,
  kind: ItemKind,
  on: boolean,
): Set<string> {
  const next = new Set(keys);
  for (const key of groupKeys(inspect, pluginName, kind)) {
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

/** Where a provider's skills land for a scope — mirrors `AgentAdapter::item_roots`. */
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

export type OutcomeSummary = {
  installed: number;
  skipped: number;
  conflicts: ItemOutcome[];
  failed: ItemOutcome[];
  touchedProviders: AgentId[];
};

export function summarizeOutcomes(result: InstallItemsResult): OutcomeSummary {
  const touched = new Set<AgentId>();
  let installed = 0;
  let skipped = 0;
  const conflicts: ItemOutcome[] = [];
  const failed: ItemOutcome[] = [];
  for (const outcome of result.outcomes) {
    switch (outcome.status) {
      case "installed":
      case "replaced":
        installed += 1;
        touched.add(outcome.provider);
        break;
      case "skipped":
        skipped += 1;
        break;
      case "conflict":
        conflicts.push(outcome);
        break;
      case "failed":
        failed.push(outcome);
        break;
    }
  }
  return { installed, skipped, conflicts, failed, touchedProviders: [...touched] };
}

/** True when nothing is left for the user to decide: no conflicts, no failures. */
export function installOutcomeClean(result: InstallItemsResult): boolean {
  const summary = summarizeOutcomes(result);
  return summary.conflicts.length === 0 && summary.failed.length === 0;
}
