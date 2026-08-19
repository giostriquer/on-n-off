import { agentRoot } from "./catalog";
import type {
  AgentId,
  ItemDependency,
  ItemKind,
  ItemPick,
  ItemScope,
  ItemTarget,
  MarketplaceEntry,
  MarketplaceInspect,
  MarketplacePlugin,
  PluginExtra,
} from "./types";

export type MarketplaceAction = "plugin" | "all" | "selected";

const KEY_SEP = " ";

export function entryKey(pluginName: string, kind: ItemKind, path: string): string {
  return [pluginName, kind, path].join(KEY_SEP);
}

export function depKey(dep: ItemDependency): string {
  return entryKey(dep.pluginName, dep.kind, dep.path);
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

/**
 * What the user has picked in the marketplace tree. `keys` is the effective selection (what
 * gets installed); `autoAdded` names the keys that were pulled in as dependencies and who
 * required each; `declined` remembers auto-adds the user unchecked so they are not re-added
 * while a parent still wants them.
 */
export type SelectionState = {
  keys: ReadonlySet<string>;
  autoAdded: ReadonlyMap<string, string[]>;
  declined: ReadonlySet<string>;
};

export function emptySelectionState(): SelectionState {
  return { keys: new Set(), autoAdded: new Map(), declined: new Set() };
}

type IndexedEntry = { key: string; plugin: MarketplacePlugin; kind: ItemKind; entry: MarketplaceEntry };

/** Supported entries by key, in marketplace order. */
function indexEntries(inspect: MarketplaceInspect): Map<string, IndexedEntry> {
  const index = new Map<string, IndexedEntry>();
  for (const plugin of inspect.plugins) {
    if (!plugin.supported) {
      continue;
    }
    for (const kind of ["skill", "agent"] as const) {
      for (const entry of kind === "skill" ? plugin.skills : plugin.agents) {
        const key = entryKey(plugin.name, kind, entry.path);
        index.set(key, { key, plugin, kind, entry });
      }
    }
  }
  return index;
}

/** Display name of every supported entry, by key. */
export function entryNames(inspect: MarketplaceInspect): Map<string, string> {
  const names = new Map<string, string>();
  for (const { key, entry } of indexEntries(inspect).values()) {
    names.set(key, entry.name);
  }
  return names;
}

/**
 * The transitive high-confidence closure of `seeds`: every key they need, directly or through
 * another dependency, mapped to the keys that require it. Seeds are never listed; `declined`
 * keys are neither listed nor traversed; targets the marketplace does not offer are ignored.
 */
export function requiredClosure(
  inspect: MarketplaceInspect,
  seeds: Iterable<string>,
  declined: ReadonlySet<string> = new Set(),
): Map<string, string[]> {
  const index = indexEntries(inspect);
  const seedSet = new Set(seeds);
  const closure = new Map<string, string[]>();
  const queue = [...seedSet];
  const visited = new Set(queue);
  while (queue.length > 0) {
    const current = queue.shift()!;
    const found = index.get(current);
    if (!found) {
      continue;
    }
    for (const dep of found.entry.dependsOn) {
      if (dep.confidence !== "high") {
        continue;
      }
      const key = depKey(dep);
      if (key === current || seedSet.has(key) || declined.has(key) || !index.has(key)) {
        continue;
      }
      const requiredBy = closure.get(key);
      if (requiredBy) {
        if (!requiredBy.includes(current)) {
          requiredBy.push(current);
        }
      } else {
        closure.set(key, [current]);
      }
      if (!visited.has(key)) {
        visited.add(key);
        queue.push(key);
      }
    }
  }
  return closure;
}

/** Explicit picks are the keys the user checked themselves. */
function explicitKeys(state: SelectionState): Set<string> {
  const explicit = new Set(state.keys);
  for (const key of state.autoAdded.keys()) {
    explicit.delete(key);
  }
  return explicit;
}

function settle(inspect: MarketplaceInspect, explicit: ReadonlySet<string>, declined: ReadonlySet<string>): SelectionState {
  const autoAdded = requiredClosure(inspect, explicit, declined);
  return { keys: new Set([...explicit, ...autoAdded.keys()]), autoAdded, declined };
}

/** Checks `key` and pulls in what it requires, minus anything the user declined earlier. */
export function checkWithDeps(state: SelectionState, inspect: MarketplaceInspect, key: string): SelectionState {
  const explicit = explicitKeys(state);
  explicit.add(key);
  const declined = new Set(state.declined);
  declined.delete(key);
  return settle(inspect, explicit, declined);
}

/**
 * Unchecks `key`. Auto-added items that lose their last requirer go with it; if something else
 * still requires `key`, the removal is remembered as declined until that parent is unchecked.
 */
export function uncheck(state: SelectionState, inspect: MarketplaceInspect, key: string): SelectionState {
  const explicit = explicitKeys(state);
  explicit.delete(key);
  const declined = new Set(state.declined);
  // Whatever this key had pulled in is released: re-checking it starts fresh.
  for (const child of requiredClosure(inspect, [key]).keys()) {
    declined.delete(child);
  }
  const next = settle(inspect, explicit, declined);
  if (!next.keys.has(key)) {
    return next;
  }
  declined.add(key);
  return settle(inspect, explicit, declined);
}

/** Select all / none for one plugin group, one key at a time through the dependency rules. */
export function toggleGroup(
  state: SelectionState,
  inspect: MarketplaceInspect,
  plugin: MarketplacePlugin,
  kind: ItemKind,
  on: boolean,
): SelectionState {
  let next = state;
  for (const key of groupKeys(plugin, kind)) {
    next = on ? checkWithDeps(next, inspect, key) : uncheck(next, inspect, key);
  }
  return next;
}

export type DependencyGap = { key: string; name: string; missing: ItemDependency[] };

/** Selected entries whose high or medium dependencies are not selected, in marketplace order. */
export function dependencyGaps(inspect: MarketplaceInspect, keys: ReadonlySet<string>): DependencyGap[] {
  const index = indexEntries(inspect);
  const gaps: DependencyGap[] = [];
  for (const { key, entry } of index.values()) {
    if (!keys.has(key)) {
      continue;
    }
    const missing = entry.dependsOn.filter((dep) => {
      const target = depKey(dep);
      return target !== key && !keys.has(target) && index.has(target);
    });
    if (missing.length > 0) {
      gaps.push({ key, name: entry.name, missing });
    }
  }
  return gaps;
}

export type PluginAdvisory = {
  extras: PluginExtra[];
  /** Selected entries that mention `CLAUDE_PLUGIN_ROOT`. */
  pluginRoot: string[];
  /** Selected entries that reach outside their own folder. */
  externalRefs: string[];
  show: boolean;
};

/** What a local copy of the selected entries of `plugin` will not carry. */
export function pluginAdvisories(plugin: MarketplacePlugin, keys: ReadonlySet<string>): PluginAdvisory {
  const pluginRoot: string[] = [];
  const externalRefs: string[] = [];
  for (const kind of ["skill", "agent"] as const) {
    for (const entry of kind === "skill" ? plugin.skills : plugin.agents) {
      if (!keys.has(entryKey(plugin.name, kind, entry.path))) {
        continue;
      }
      if (entry.usesPluginRoot) {
        pluginRoot.push(entry.name);
      }
      if (entry.externalRefs.length > 0) {
        externalRefs.push(entry.name);
      }
    }
  }
  const extras = plugin.supported ? plugin.extras : [];
  return {
    extras,
    pluginRoot,
    externalRefs,
    show: extras.length > 0 || pluginRoot.length > 0 || externalRefs.length > 0,
  };
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
