import { describe, expect, it } from "vitest";
import {
  agentsAllowed,
  allKeys,
  entryKey,
  installOutcomeClean,
  previewPath,
  selectedItems,
  summarizeOutcomes,
  toggleGroup,
} from "./marketplaceSelection";
import type { InstallItemsResult, MarketplaceInspect } from "./types";

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
        { name: "tdd", description: "Test-driven development", path: "skills/engineering/tdd" },
        { name: "grilling", description: "Ask hard questions", path: "skills/productivity/grilling" },
      ],
      agents: [{ name: "reviewer", description: "Reviews code", path: "agents/reviewer.md" }],
    },
    {
      name: "remote",
      version: null,
      description: "",
      supported: false,
      source: null,
      skills: [],
      agents: [],
    },
    {
      name: "gh",
      version: null,
      description: "",
      supported: true,
      source: { owner: "other", repo: "repo", ref: "HEAD" },
      skills: [{ name: "gamma", description: "", path: "skills/gamma" }],
      agents: [],
    },
  ],
};

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
    const one = new Set([entryKey("gh", "skill", "skills/gamma")]);
    const on = toggleGroup(one, inspect, "mattpocock-skills", "skill", true);
    expect(on.size).toBe(3);
    const off = toggleGroup(on, inspect, "mattpocock-skills", "skill", false);
    expect([...off]).toEqual([entryKey("gh", "skill", "skills/gamma")]);
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

  it("summarises outcomes and knows when the sheet can close", () => {
    const result: InstallItemsResult = {
      commitSha: "b".repeat(40),
      shaMoved: false,
      outcomes: [
        { provider: "claude", kind: "skill", name: "tdd", targetPath: "x", status: "installed", reason: null },
        { provider: "codex", kind: "skill", name: "tdd", targetPath: "y", status: "replaced", reason: null },
        { provider: "codex", kind: "agent", name: "reviewer", targetPath: "", status: "skipped", reason: "n/a" },
        { provider: "claude", kind: "skill", name: "grilling", targetPath: "z", status: "conflict", reason: "exists" },
      ],
    };
    const summary = summarizeOutcomes(result);
    expect(summary.installed).toBe(2);
    expect(summary.conflicts.map((o) => o.name)).toEqual(["grilling"]);
    expect(summary.failed).toEqual([]);
    expect(summary.touchedProviders).toEqual(["claude", "codex"]);
    expect(installOutcomeClean(result)).toBe(false);
    expect(installOutcomeClean({ ...result, outcomes: result.outcomes.slice(0, 3) })).toBe(true);
  });
});
