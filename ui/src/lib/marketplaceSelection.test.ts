import { describe, expect, it } from "vitest";
import {
  agentsAllowed,
  allKeys,
  checkWithDeps,
  dependencyGaps,
  depKey,
  emptySelectionState,
  entryKey,
  pluginAdvisories,
  previewPath,
  requiredClosure,
  selectedItems,
  toggleGroup,
  uncheck,
  type SelectionState,
} from "./marketplaceSelection";
import type { ItemDependency, MarketplaceEntry, MarketplaceInspect } from "./types";

function entry(name: string, path: string, extra: Partial<MarketplaceEntry> = {}): MarketplaceEntry {
  return { name, description: "", path, dependsOn: [], externalRefs: [], usesPluginRoot: false, ...extra };
}

function needs(name: string, path: string, confidence: "high" | "medium" = "high", pluginName = "acme"): ItemDependency {
  return { pluginName, kind: "skill", path, name, confidence };
}

const inspect: MarketplaceInspect = {
  isMarketplace: true,
  commitSha: "a".repeat(40),
  marketplaceName: "mattpocock",
  hint: null,
  plugins: [
    {
      name: "mattpocock-skills",
      version: "1.2.3",
      description: "Matt's skills",
      supported: true,
      source: null,
      skills: [
        entry("tdd", "skills/engineering/tdd", { description: "Test-driven development" }),
        entry("grilling", "skills/productivity/grilling", { description: "Ask hard questions" }),
      ],
      agents: [entry("reviewer", "agents/reviewer.md", { description: "Reviews code" })],
      extras: [],
    },
    {
      name: "remote",
      version: null,
      description: "",
      supported: false,
      source: null,
      skills: [],
      agents: [],
      extras: [],
    },
    {
      name: "gh",
      version: null,
      description: "",
      supported: true,
      source: { owner: "other", repo: "repo", ref: "HEAD" },
      skills: [entry("gamma", "skills/gamma")],
      agents: [],
      extras: [],
    },
  ],
};

/** Invented names: router -> build -> verify, review; build also mentions probe loosely. */
const deps: MarketplaceInspect = {
  ...inspect,
  plugins: [
    {
      name: "acme",
      version: null,
      description: "",
      supported: true,
      source: null,
      skills: [
        entry("router", "skills/router", { dependsOn: [needs("build", "skills/build")] }),
        entry("build", "skills/build", {
          dependsOn: [
            needs("verify", "skills/verify"),
            needs("review", "skills/review"),
            needs("probe", "skills/probe", "medium"),
            // A dependency the marketplace no longer lists is ignored, never crashes.
            needs("ghost", "skills/ghost"),
          ],
        }),
        entry("verify", "skills/verify", { dependsOn: [needs("review", "skills/review")] }),
        entry("review", "skills/review", { dependsOn: [needs("verify", "skills/verify", "medium")] }),
        entry("probe", "skills/probe", { usesPluginRoot: true }),
        entry("guide", "skills/guide", { externalRefs: ["../lib/steps.md"] }),
      ],
      agents: [],
      extras: ["commands", "mcp"],
    },
    {
      name: "unsupported",
      version: null,
      description: "",
      supported: false,
      source: null,
      skills: [entry("verify", "skills/verify")],
      agents: [],
      extras: [],
    },
  ],
};

const K = (name: string, plugin = "acme") => entryKey(plugin, "skill", `skills/${name}`);

function state(keys: string[], autoAdded: [string, string[]][] = [], declined: string[] = []): SelectionState {
  return { keys: new Set(keys), autoAdded: new Map(autoAdded), declined: new Set(declined) };
}

describe("marketplaceSelection", () => {
  it("lists every supported entry and maps keys back to picks with their source", () => {
    const keys = allKeys(inspect);
    expect(keys).toHaveLength(4);
    const picks = selectedItems(inspect, new Set(keys));
    expect(picks).toEqual([
      { pluginName: "mattpocock-skills", kind: "skill", path: "skills/engineering/tdd", source: null },
      { pluginName: "mattpocock-skills", kind: "skill", path: "skills/productivity/grilling", source: null },
      { pluginName: "mattpocock-skills", kind: "agent", path: "agents/reviewer.md", source: null },
      { pluginName: "gh", kind: "skill", path: "skills/gamma", source: { owner: "other", repo: "repo", ref: "HEAD" } },
    ]);
  });

  it("toggles a plugin group on and off without touching other groups", () => {
    const one = state([entryKey("gh", "skill", "skills/gamma")]);
    const matt = inspect.plugins[0];
    const on = toggleGroup(one, inspect, matt, "skill", true);
    expect(on.keys.size).toBe(3);
    const off = toggleGroup(on, inspect, matt, "skill", false);
    expect([...off.keys]).toEqual([entryKey("gh", "skill", "skills/gamma")]);
  });

  it("only allows agents when Claude is a target and previews provider paths", () => {
    expect(agentsAllowed(["codex"])).toBe(false);
    expect(agentsAllowed(["codex", "claude"])).toBe(true);
    expect(previewPath("claude", { kind: "global" })).toBe("~/.claude/skills");
    expect(previewPath("antigravity", { kind: "global" })).toBe("~/.gemini/antigravity-cli/skills");
    expect(previewPath("codex", { kind: "project", projectPath: "E:/dev/app" })).toBe("E:/dev/app/.codex/skills");
    expect(previewPath("cursor", { kind: "project", projectPath: "/home/me/app" })).toBe(
      "/home/me/app/.cursor/skills",
    );
  });

  it("closes over high-confidence dependencies transitively and records who required what", () => {
    expect(depKey(needs("verify", "skills/verify"))).toBe(K("verify"));
    const closure = requiredClosure(deps, [K("router")]);
    expect([...closure.entries()]).toEqual([
      [K("build"), [K("router")]],
      [K("verify"), [K("build")]],
      [K("review"), [K("build"), K("verify")]],
    ]);
    // Seeds are never their own dependency; medium mentions and unknown targets are skipped.
    expect(requiredClosure(deps, [K("verify"), K("review")]).size).toBe(0);
    expect(requiredClosure(deps, [K("probe")]).size).toBe(0);
    // Declined keys are neither added nor traversed.
    const partial = requiredClosure(deps, [K("router")], new Set([K("build")]));
    expect([...partial.keys()]).toEqual([]);
  });

  it("auto-adds required items on check, labels them, and cascades on uncheck", () => {
    const after = checkWithDeps(emptySelectionState(), deps, K("build"));
    expect([...after.keys].sort()).toEqual([K("review"), K("build"), K("verify")].sort());
    expect(after.autoAdded.get(K("verify"))).toEqual([K("build")]);
    expect(after.autoAdded.get(K("review"))).toEqual([K("build"), K("verify")]);
    expect(after.autoAdded.has(K("build"))).toBe(false);
    expect(after.declined.size).toBe(0);

    // Explicitly checking an auto-added item promotes it: it no longer follows its parent.
    const promoted = checkWithDeps(after, deps, K("verify"));
    expect(promoted.autoAdded.has(K("verify"))).toBe(false);
    const parentGone = uncheck(promoted, deps, K("build"));
    expect([...parentGone.keys].sort()).toEqual([K("review"), K("verify")].sort());
    expect(parentGone.autoAdded.get(K("review"))).toEqual([K("verify")]);

    // Unchecking a parent drops what only it required.
    const cascade = uncheck(after, deps, K("build"));
    expect(cascade.keys.size).toBe(0);
    expect(cascade.autoAdded.size).toBe(0);
    expect(cascade.declined.size).toBe(0);
  });

  it("respects a declined auto-add until its parent is re-checked", () => {
    const withBuild = checkWithDeps(emptySelectionState(), deps, K("build"));
    const declined = uncheck(withBuild, deps, K("verify"));
    expect(declined.keys.has(K("verify"))).toBe(false);
    expect(declined.declined.has(K("verify"))).toBe(true);
    // review stays: build still requires it directly.
    expect(declined.keys.has(K("review"))).toBe(true);
    expect(declined.autoAdded.get(K("review"))).toEqual([K("build")]);

    // Another parent arriving does not override the user's "no".
    const more = checkWithDeps(declined, deps, K("router"));
    expect(more.keys.has(K("verify"))).toBe(false);
    expect(more.declined.has(K("verify"))).toBe(true);

    // Unchecking and re-checking the parent brings the dependency back.
    const off = uncheck(more, deps, K("build"));
    expect(off.declined.has(K("verify"))).toBe(false);
    const on = checkWithDeps(off, deps, K("build"));
    expect(on.keys.has(K("verify"))).toBe(true);
    expect(on.autoAdded.get(K("verify"))).toEqual([K("build")]);

    // Checking the declined item itself is always honoured.
    const explicit = checkWithDeps(declined, deps, K("verify"));
    expect(explicit.keys.has(K("verify"))).toBe(true);
    expect(explicit.declined.has(K("verify"))).toBe(false);
    expect(explicit.autoAdded.has(K("verify"))).toBe(false);
  });

  it("selects a whole group through the same dependency rules", () => {
    const plugin = deps.plugins[0];
    const on = toggleGroup(emptySelectionState(), deps, plugin, "skill", true);
    expect(on.keys.size).toBe(6);
    expect(on.autoAdded.size).toBe(0);
    const off = toggleGroup(on, deps, plugin, "skill", false);
    expect(off.keys.size).toBe(0);
    expect(off.declined.size).toBe(0);
  });

  it("reports dependency gaps for high and medium mentions that are not selected", () => {
    const keys = new Set([K("build"), K("review")]);
    const gaps = dependencyGaps(deps, keys);
    expect(gaps).toEqual([
      {
        key: K("build"),
        name: "build",
        missing: [needs("verify", "skills/verify"), needs("probe", "skills/probe", "medium")],
      },
      { key: K("review"), name: "review", missing: [needs("verify", "skills/verify", "medium")] },
    ]);
    expect(dependencyGaps(deps, new Set([K("build"), K("verify"), K("review"), K("probe")]))).toEqual([]);
  });

  it("summarises what a local copy of the selected entries will not carry", () => {
    const plugin = deps.plugins[0];
    expect(pluginAdvisories(plugin, new Set([K("verify")]))).toEqual({
      extras: ["commands", "mcp"],
      pluginRoot: [],
      externalRefs: [],
      show: true,
    });
    expect(pluginAdvisories({ ...plugin, extras: [] }, new Set([K("verify")]))).toEqual({
      extras: [],
      pluginRoot: [],
      externalRefs: [],
      show: false,
    });
    expect(pluginAdvisories({ ...plugin, extras: [] }, new Set([K("probe"), K("guide")]))).toEqual({
      extras: [],
      pluginRoot: ["probe"],
      externalRefs: ["guide"],
      show: true,
    });
  });
});
